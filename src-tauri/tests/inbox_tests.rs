use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::inbox::{clear_inbox_inner, set_inbox_path_from_path};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{
    canonicalize_for_authorization, canonicalize_root, AuthorizedPaths,
};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

fn make_state(dir: &TempDir) -> AppState {
    let writ_dir = dir.path().to_path_buf();
    let buffers_dir = writ_dir.join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");

    let notes_root = writ_dir.join("Writ");
    std::fs::create_dir_all(&notes_root).expect("notes folder");
    let notes_root = writ_tauri_lib::security::canonicalize_root(&notes_root).expect("canonical");

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
        notes_root: RwLock::new(notes_root),
        notes_root_fallback: RwLock::new(None),
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        notes_watcher: Mutex::new(None),
        open_file_watcher: Mutex::new(None),
        file_tracking: Mutex::new(None),
        notes_index: Arc::new(NotesIndexStore::open(&db_path).expect("notes index db")),
        notes_index_cancel: Arc::new(AtomicBool::new(false)),
        notes_reconcile: Arc::new(ReconcileGate::new()),
        quit: Arc::new(QuitState::new()),
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
        last_disk_hash: Mutex::new(std::collections::HashMap::new()),
        source_records: Mutex::new(std::collections::HashMap::new()),
        unsaved_on_exit: Mutex::new(std::collections::HashMap::new()),
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

    assert_eq!(
        root,
        writ_tauri_lib::security::canonicalize_root(inbox.path())
            .unwrap()
            .to_string_lossy()
    );
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

/// Opens the file at `path` through the command layer and saves `content` into
/// it, which is the only way a save reaches the ignore set.
fn save_through_writ(state: &AppState, path: &std::path::Path, content: &str) {
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(state, &canonical).expect("open");
    save_buffer_content_inner(
        state,
        &opened.doc.as_ref().expect("the file opened").id,
        content,
    )
    .expect("save");
}

fn classify_arrival(
    state: &AppState,
    path: &std::path::Path,
    root: &std::path::Path,
) -> Option<writ_core::events::bus::WritEvent> {
    writ_tauri_lib::watcher::handler::classify_inbox_event(
        path,
        root,
        &std::collections::HashSet::new(),
        &state.watcher_ignore,
        writ_core::watcher::ignore::DEFAULT_IGNORE_TTL,
        std::time::Instant::now(),
    )
}

#[test]
fn a_writ_save_of_notes_a_index_md_does_not_suppress_an_external_change_to_notes_b_index_md() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let watched = TempDir::new().unwrap();
    let root = canonicalize_root(watched.path()).expect("canonical");
    std::fs::create_dir_all(root.join("a")).unwrap();
    std::fs::create_dir_all(root.join("b")).unwrap();

    let a = root.join("a").join("index.md");
    std::fs::write(&a, "what a held").unwrap();
    save_through_writ(&state, &a, "what writ wrote");

    // b holds exactly the bytes Writ wrote into a, so the fingerprint cannot
    // tell the two files apart. Only the key can, and a bare `index.md` key
    // was shared by both.
    let b = root.join("b").join("index.md");
    std::fs::write(&b, "what writ wrote").unwrap();

    assert!(
        classify_arrival(&state, &b, &root).is_some(),
        "a save of a/index.md must not swallow b/index.md"
    );
    assert!(
        classify_arrival(&state, &a, &root).is_none(),
        "the file Writ saved is still its own write"
    );
}

#[test]
fn one_writ_save_fanning_out_into_several_events_is_fully_suppressed() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let watched = TempDir::new().unwrap();
    let root = canonicalize_root(watched.path()).expect("canonical");

    let note = root.join("agent-output.md");
    std::fs::write(&note, "# from somewhere").unwrap();
    save_through_writ(&state, &note, "# edited in writ");

    for attempt in 1..=3 {
        assert!(
            classify_arrival(&state, &note, &root).is_none(),
            "event {attempt} of one write must stay suppressed"
        );
    }
}

#[test]
fn editing_a_file_named_config_toml_in_the_watched_folder_does_not_suppress_a_real_config_reload() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let watched = TempDir::new().unwrap();
    let root = canonicalize_root(watched.path()).expect("canonical");

    let note = root.join("config.toml");
    std::fs::write(&note, "theme = \"dark\"\n").unwrap();
    save_through_writ(&state, &note, "theme = \"light\"\n");

    // The config file now holds what Writ wrote into the note, so the bytes
    // match the stamp and the namespace is all that separates them.
    let config_path = state.config_store.path().to_path_buf();
    std::fs::write(&config_path, "theme = \"light\"\n").unwrap();

    let event = writ_tauri_lib::watcher::handler::classify_watch_event(
        &config_path,
        &config_path,
        &state.watcher_ignore,
        writ_core::watcher::ignore::DEFAULT_IGNORE_TTL,
        std::time::Instant::now(),
    );
    assert!(
        matches!(
            event,
            Some(writ_core::events::bus::WritEvent::ConfigChanged { .. })
        ),
        "a note named config.toml must not stand in for the config file: {event:?}"
    );
}

#[test]
fn a_writ_config_write_does_not_suppress_an_external_edit_to_config_toml_in_the_watched_folder() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let watched = TempDir::new().unwrap();
    let root = canonicalize_root(watched.path()).expect("canonical");

    // Setting the folder persists the config, which stamps the data folder's
    // own config.toml.
    set_inbox_path_from_path(&state, &root).expect("set inbox");
    let written = std::fs::read(state.config_store.path()).expect("read the config back");

    // Somebody else's file of the same name holding the same bytes: the
    // fingerprint matches, so only the key namespace tells the two apart.
    let lookalike = root.join("config.toml");
    std::fs::write(&lookalike, &written).unwrap();

    assert!(
        classify_arrival(&state, &lookalike, &root).is_some(),
        "a config write must not swallow an edit to a config.toml in the watched folder"
    );
}
