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
use writ_tauri_lib::commands::file::{open_file_from_path, save_to_source_for_test};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
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
        workspace_index: Arc::new(RwLock::new(
            writ_tauri_lib::workspace_index::WorkspaceIndex::new(None),
        )),
        search_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    }
}

#[test]
fn open_file_allows_path_inside_open_workspace() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let ws = TempDir::new().unwrap();
    let note = ws.path().join("note.md");
    std::fs::write(&note, "workspace file").unwrap();

    {
        let mut root = state.workspace_root.lock().unwrap();
        *root = Some(writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap());
    }

    let result = open_file_from_path(&state, &note.to_string_lossy());
    assert!(
        result.is_ok(),
        "workspace-contained open must pass: {result:?}"
    );
}

#[test]
fn open_file_still_rejects_path_outside_open_workspace() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let ws = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "shhh").unwrap();

    {
        let mut root = state.workspace_root.lock().unwrap();
        *root = Some(writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap());
    }

    let result = open_file_from_path(&state, &secret.to_string_lossy());
    assert!(result.is_err(), "outside-workspace open must stay rejected");
}

#[test]
fn open_file_rejects_path_that_was_never_authorized() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "shhh").unwrap();
    let secret_str = secret.to_str().unwrap();

    let result = open_file_from_path(&state, secret_str);
    assert!(result.is_err(), "expected gate to reject unauthorized open");
    assert!(
        result.as_ref().unwrap_err().contains("not authorized"),
        "unexpected error: {:?}",
        result
    );

    let store = state.store.lock().unwrap();
    let active = store
        .list_by_status(writ_core::buffer::document::BufferStatus::Active)
        .unwrap();
    assert!(
        active.is_empty(),
        "no buffer should have been created for an unauthorized path"
    );
}

#[test]
fn open_file_accepts_explicitly_authorized_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("notes.md");
    std::fs::write(&file, "# hi").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    let result = open_file_from_path(&state, &canonical).expect("authorized open should succeed");
    assert_eq!(result.doc.source_path.as_deref(), Some(canonical.as_str()));
}

#[test]
fn open_file_authorization_is_single_use() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("once.txt");
    std::fs::write(&file, "first content").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    let first = open_file_from_path(&state, &canonical);
    assert!(first.is_ok(), "first open should succeed: {:?}", first);

    let second = open_file_from_path(&state, &canonical);
    assert!(
        second.is_err(),
        "second open should require fresh authorization"
    );
    assert!(second.unwrap_err().contains("not authorized"));
}

#[test]
fn open_file_blesses_source_path_for_subsequent_saves() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("save.md");
    std::fs::write(&file, "alpha").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    let result = open_file_from_path(&state, &canonical).expect("open");

    save_to_source_for_test(&state, result.doc.id.clone(), "beta".to_string())
        .expect("save should succeed for blessed source");

    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert_eq!(on_disk, "beta");
}

#[test]
fn save_to_source_rejects_unblessed_source_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("planted.md");
    std::fs::write(&file, "original").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();

    let store = state.store.lock().unwrap();
    let mut mgr = writ_core::buffer::manager::BufferManager::new();
    let doc = mgr.open_external(canonical.clone()).expect("mint");
    store.open_from_path(&doc, "original").expect("persist");
    drop(store);

    let result = save_to_source_for_test(&state, doc.id, "hijacked".to_string());
    assert!(
        result.is_err(),
        "unblessed source path must not be writable"
    );
    assert!(result.unwrap_err().contains("not authorized"));

    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert_eq!(on_disk, "original", "file must not have been overwritten");
}

#[test]
fn open_file_for_active_duplicate_blesses_without_consuming_again() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("dup.md");
    std::fs::write(&file, "x").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    let first = open_file_from_path(&state, &canonical).expect("first open");

    state.authorized_paths.record_for_open(canonical.clone());
    let second = open_file_from_path(&state, &canonical).expect("second open returns existing");
    assert_eq!(first.doc.id, second.doc.id);

    save_to_source_for_test(&state, second.doc.id, "y".to_string()).expect("save");
}
