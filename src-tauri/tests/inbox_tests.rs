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
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::inbox::{clear_inbox_inner, set_inbox_path_from_path};
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

fn collect_arrivals(state: &AppState) -> Arc<Mutex<Vec<String>>> {
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    state.event_bus.subscribe(move |event| {
        if let writ_core::events::bus::WritEvent::InboxFileArrived { path } = event {
            received_clone.lock().unwrap().push(path.clone());
        }
    });
    received
}

#[test]
fn set_inbox_canonicalizes_persists_and_starts_watcher() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();

    let root = set_inbox_path_from_path(&state, inbox.path()).expect("set inbox");

    assert_eq!(root, inbox.path().canonicalize().unwrap().to_string_lossy());
    assert!(state.inbox_watcher.lock().unwrap().is_some());
    assert_eq!(
        state.config.lock().unwrap().inbox.path.as_deref(),
        Some(root.as_str())
    );

    let persisted = state.config_store.read().expect("read config back");
    assert_eq!(persisted.inbox.path.as_deref(), Some(root.as_str()));
    assert!(persisted.inbox.focus, "focus default must survive persist");
}

#[test]
fn set_inbox_rejects_file_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();
    let file = inbox.path().join("a.txt");
    std::fs::write(&file, "x").unwrap();

    assert!(set_inbox_path_from_path(&state, &file).is_err());
}

#[test]
fn set_inbox_rejects_missing_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    assert!(
        set_inbox_path_from_path(&state, std::path::Path::new("/nonexistent/writ-inbox")).is_err()
    );
}

#[test]
fn clear_inbox_drops_state_watcher_and_config() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();

    set_inbox_path_from_path(&state, inbox.path()).expect("set inbox");
    clear_inbox_inner(&state).expect("clear inbox");

    assert!(state.inbox_root.lock().unwrap().is_none());
    assert!(state.inbox_watcher.lock().unwrap().is_none());
    assert!(state.config.lock().unwrap().inbox.path.is_none());

    let persisted = state.config_store.read().expect("read config back");
    assert!(persisted.inbox.path.is_none());
}

#[test]
fn inbox_watcher_emits_arrival_on_bus_for_new_file() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();
    let received = collect_arrivals(&state);

    set_inbox_path_from_path(&state, inbox.path()).expect("set inbox");

    let file = inbox.path().join("report.md");
    std::fs::write(&file, "# finished").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if received
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.ends_with("report.md"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "inbox arrival event never arrived"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn inbox_watcher_never_emits_for_preexisting_backlog() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();

    let backlog = inbox.path().join("old-report.md");
    std::fs::write(&backlog, "stale").unwrap();

    let received = collect_arrivals(&state);
    set_inbox_path_from_path(&state, inbox.path()).expect("set inbox");

    std::fs::write(&backlog, "stale but touched").unwrap();

    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(
        received.lock().unwrap().is_empty(),
        "pre-existing files must never auto-open, even when modified"
    );
}

#[test]
fn open_file_allows_path_inside_inbox() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();
    let note = inbox.path().join("arrived.md");
    std::fs::write(&note, "inbox file").unwrap();

    set_inbox_path_from_path(&state, inbox.path()).expect("set inbox");

    let result = open_file_from_path(&state, &note.to_string_lossy());
    assert!(result.is_ok(), "inbox-contained open must pass: {result:?}");
}

#[test]
fn open_file_still_rejects_path_outside_inbox() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "shhh").unwrap();

    set_inbox_path_from_path(&state, inbox.path()).expect("set inbox");

    let result = open_file_from_path(&state, &secret.to_string_lossy());
    assert!(result.is_err(), "outside-inbox open must stay rejected");
}

#[test]
fn open_file_rejects_inbox_path_after_clear() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let inbox = TempDir::new().unwrap();
    let note = inbox.path().join("late.md");
    std::fs::write(&note, "x").unwrap();

    set_inbox_path_from_path(&state, inbox.path()).expect("set inbox");
    clear_inbox_inner(&state).expect("clear inbox");

    let result = open_file_from_path(&state, &note.to_string_lossy());
    assert!(
        result.is_err(),
        "clearing the inbox must revoke folder-derived authorization"
    );
}
