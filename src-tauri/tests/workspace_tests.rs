use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_tauri_lib::commands::workspace::{
    clear_workspace_root_inner, list_workspace_dir_inner, set_workspace_root_from_path,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

fn make_state(dir: &TempDir) -> AppState {
    let writ_dir = dir.path().to_path_buf();
    let buffers_dir = writ_dir.join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");

    let db_path = writ_dir.join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let store = BufferStore::new(conn, buffers_dir.clone());

    let config_path = writ_dir.join("config.toml");
    let config_store = ConfigStore::new(config_path);

    AppState {
        store: Mutex::new(store),
        config_store,
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        pending_opens: Mutex::new(Vec::new()),
        frontend_ready: AtomicBool::new(false),
        transforms: RwLock::new(TransformRegistry::new()),
        event_bus: Arc::new(EventBus::new()),
        update_phase: Mutex::new(UpdatePhase::default()),
        authorized_paths: AuthorizedPaths::new(),
        preview_registry: Arc::new(RwLock::new(ContentRendererRegistry::new())),
        preview_render_cache: Arc::new(RenderCache::new()),
        layout_state: LayoutStateStore::new(open_database(&db_path).expect("layout db")),
        recovered_buffers: Mutex::new(Vec::new()),
        was_dirty_shutdown: false,
        workspace_root: Mutex::new(None),
        workspace_watcher: Mutex::new(None),
        inbox_root: Mutex::new(None),
        inbox_watcher: Mutex::new(None),
        fts_scheduler: writ_tauri_lib::fts_scheduler::FtsScheduler::new(),
    }
}

#[test]
fn set_root_canonicalizes_persists_and_starts_watcher() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let ws = TempDir::new().unwrap();

    let root = set_workspace_root_from_path(&state, ws.path()).expect("set root");

    assert_eq!(root, ws.path().canonicalize().unwrap().to_string_lossy());
    assert!(state.workspace_watcher.lock().unwrap().is_some());
    assert_eq!(
        state.config.lock().unwrap().workspace.root.as_deref(),
        Some(root.as_str())
    );

    let persisted = state.config_store.read().expect("read config back");
    assert_eq!(persisted.workspace.root.as_deref(), Some(root.as_str()));
}

#[test]
fn set_root_rejects_file_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let ws = TempDir::new().unwrap();
    let file = ws.path().join("a.txt");
    std::fs::write(&file, "x").unwrap();

    assert!(set_workspace_root_from_path(&state, &file).is_err());
}

#[test]
fn set_root_rejects_missing_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    assert!(set_workspace_root_from_path(&state, std::path::Path::new("/nonexistent/writ-ws")).is_err());
}

#[test]
fn clear_root_drops_state_watcher_and_config() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let ws = TempDir::new().unwrap();

    set_workspace_root_from_path(&state, ws.path()).expect("set root");
    clear_workspace_root_inner(&state).expect("clear root");

    assert!(state.workspace_root.lock().unwrap().is_none());
    assert!(state.workspace_watcher.lock().unwrap().is_none());
    assert!(state.config.lock().unwrap().workspace.root.is_none());

    let persisted = state.config_store.read().expect("read config back");
    assert!(persisted.workspace.root.is_none());
}

#[test]
fn list_dir_without_root_errors() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    assert!(list_workspace_dir_inner(&state, "/tmp").is_err());
}

#[test]
fn list_dir_inside_root_returns_sorted_entries() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let ws = TempDir::new().unwrap();
    std::fs::create_dir(ws.path().join("src")).unwrap();
    std::fs::write(ws.path().join("a.md"), "x").unwrap();
    std::fs::create_dir(ws.path().join("node_modules")).unwrap();

    let root = set_workspace_root_from_path(&state, ws.path()).expect("set root");

    let entries = list_workspace_dir_inner(&state, &root).expect("list");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["src", "a.md"]);
}

#[test]
fn list_dir_outside_root_is_rejected() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let ws = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();

    set_workspace_root_from_path(&state, ws.path()).expect("set root");

    assert!(list_workspace_dir_inner(&state, &outside.path().to_string_lossy()).is_err());
}

#[test]
fn workspace_watcher_emits_change_events_on_bus() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let ws = TempDir::new().unwrap();

    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    state.event_bus.subscribe(move |event| {
        if let writ_core::events::bus::WritEvent::WorkspaceChanged { path, .. } = event {
            received_clone.lock().unwrap().push(path.clone());
        }
    });

    set_workspace_root_from_path(&state, ws.path()).expect("set root");

    let file = ws.path().join("fresh.md");
    std::fs::write(&file, "hello").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if received
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.ends_with("fresh.md"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "workspace change event never arrived"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
