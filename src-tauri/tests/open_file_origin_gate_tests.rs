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
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::commands::file::open_file_from_path;
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

    save_buffer_content_inner(&state, &result.doc.id, "beta")
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

    let result = save_buffer_content_inner(&state, &doc.id, "hijacked");
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

    save_buffer_content_inner(&state, &second.doc.id, "y").expect("save");
}

#[test]
fn autosave_writes_the_file_the_buffer_was_opened_from() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("aliases.zsh");
    std::fs::write(&file, "alias a=b\n").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    // The regression: every save route ends here, and for a file-backed buffer
    // it used to write only Writ's private copy, so edits never reached the
    // file the user had opened.
    save_buffer_content_inner(&state, &opened.doc.id, "alias a=c\n").expect("save");

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "alias a=c\n");

    let mirror = dir.path().join("buffers").join(&opened.doc.filename);
    assert_eq!(
        std::fs::read_to_string(mirror).unwrap(),
        "alias a=c\n",
        "the copy Writ reads back has to track the file"
    );
}

#[test]
fn autosave_of_a_scratch_buffer_stays_in_the_buffers_dir() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let doc = {
        let store = state.store.lock().unwrap();
        let mut mgr = writ_core::buffer::manager::BufferManager::new();
        let doc = mgr.create_buffer(None).expect("mint");
        store.insert(&doc).expect("persist");
        doc
    };

    save_buffer_content_inner(&state, &doc.id, "just notes").expect("save");

    let mirror = dir.path().join("buffers").join(&doc.filename);
    assert_eq!(std::fs::read_to_string(mirror).unwrap(), "just notes");
}

#[test]
fn autosave_recreates_a_file_deleted_underneath_the_buffer() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("gone.md");
    std::fs::write(&file, "here").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    std::fs::remove_file(&file).unwrap();

    // The gate re-resolves the path on every save; a deleted file has nothing
    // to resolve, and refusing there would turn autosave into a permanent
    // failure instead of writing the buffer back out.
    save_buffer_content_inner(&state, &opened.doc.id, "back").expect("save");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "back");
}

#[cfg(unix)]
#[test]
fn autosave_keeps_the_files_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("run.sh");
    std::fs::write(&file, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    save_buffer_content_inner(&state, &opened.doc.id, "#!/bin/sh\necho hi\n").expect("save");

    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o755,
        "editing a script must not make it unexecutable"
    );
}

#[test]
fn reopening_a_file_edited_elsewhere_resyncs_the_buffer() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("shared.md");
    std::fs::write(&file, "writ version").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    std::fs::write(&file, "someone else's version").unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    let reopened = open_file_from_path(&state, &canonical).expect("reopen");
    assert_eq!(reopened.doc.id, opened.doc.id, "same tab, not a second one");

    let store = state.store.lock().unwrap();
    assert_eq!(
        store.read_content(&opened.doc.id).unwrap(),
        "someone else's version",
        "reopening shows the file, not the copy Writ loaded earlier"
    );
}

#[test]
fn reopening_an_untouched_file_leaves_the_buffer_alone() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("steady.md");
    std::fs::write(&file, "unchanged").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    let mirror = dir.path().join("buffers").join(&opened.doc.filename);
    let before = std::fs::metadata(&mirror).unwrap().modified().unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen");

    assert_eq!(
        std::fs::metadata(&mirror).unwrap().modified().unwrap(),
        before,
        "nothing changed on disk, so nothing should have been rewritten"
    );
}

#[test]
fn a_tab_restored_after_a_restart_can_still_be_saved() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("restored.md");
    std::fs::write(&file, "before the restart").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    // Blessing lives in memory, so a relaunch starts from the persisted rows.
    // If the rehydrated key did not match what a save presents, every autosave
    // after a restart would fail as an unauthorized write.
    let restarted = make_state(&dir);
    {
        let store = restarted.store.lock().unwrap();
        let hydrated =
            writ_tauri_lib::state::bless_persisted_sources(&store, &restarted.authorized_paths);
        assert_eq!(hydrated, 1);
    }

    save_buffer_content_inner(&restarted, &opened.doc.id, "after the restart").expect("save");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "after the restart");
}

#[test]
fn a_restored_tab_whose_file_was_deleted_recreates_it() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("deleted-while-closed.md");
    std::fs::write(&file, "content").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    std::fs::remove_file(&file).unwrap();

    let restarted = make_state(&dir);
    {
        let store = restarted.store.lock().unwrap();
        writ_tauri_lib::state::bless_persisted_sources(&store, &restarted.authorized_paths);
    }

    save_buffer_content_inner(&restarted, &opened.doc.id, "rewritten").expect("save");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "rewritten");
}
