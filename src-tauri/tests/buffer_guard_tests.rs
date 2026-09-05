use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::buffer::manager::BufferManager;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::hash::sha256_bytes;
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
use writ_tauri_lib::commands::buffer::{
    decide_create_buffer, read_buffer_content_inner, save_buffer_content_inner,
    save_failure_message, CreateDecision, ERR_FILE_CHANGED_ON_DISK, ERR_FILE_IN_USE,
    ERR_FILE_NOT_DOWNLOADED, ERR_HARD_LINKED, ERR_READ_ONLY_DESTINATION, ERR_WRITE_FAILED,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    (dir, BufferStore::new(conn, buffers_dir))
}

/// Persists a new note the way `create_buffer` does: mint via the manager and
/// insert the row. Nothing is written to disk — a note reaches a file on its
/// first keystroke (ADR-028 §2), and a note that has none is exactly the one a
/// new-tab request reuses.
fn persist_empty_scratch(store: &BufferStore, mgr: &mut BufferManager) -> String {
    let doc = mgr.create_buffer(None).expect("mint");
    store.insert(&doc).expect("insert");
    doc.id
}

#[test]
fn create_buffer_reuses_existing_empty_scratch() {
    let (_dir, store) = setup();
    let mut mgr = BufferManager::new();
    let existing_id = persist_empty_scratch(&store, &mut mgr);

    let decision = decide_create_buffer(&store, &mut mgr, None).expect("decide");
    match decision {
        CreateDecision::Reuse(doc) => assert_eq!(doc.id, existing_id),
        CreateDecision::Create(_) => panic!("expected reuse of empty scratch, got create"),
    }
}

#[test]
fn create_buffer_mints_new_when_no_empty_scratch() {
    let (_dir, store) = setup();
    let mut mgr = BufferManager::new();

    let decision = decide_create_buffer(&store, &mut mgr, None).expect("decide");
    assert!(matches!(decision, CreateDecision::Create(_)));
}

#[test]
fn create_buffer_with_explicit_title_always_mints() {
    let (_dir, store) = setup();
    let mut mgr = BufferManager::new();
    persist_empty_scratch(&store, &mut mgr);

    let decision =
        decide_create_buffer(&store, &mut mgr, Some("Named".to_string())).expect("decide");
    match decision {
        CreateDecision::Create(doc) => assert_eq!(doc.title, "Named"),
        CreateDecision::Reuse(_) => panic!("explicit title must never reuse"),
    }
}

/// A whole app state over a temp data folder, which the guard needs because
/// what a save compares against lives on it, not in the store.
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

    AppState {
        store: Mutex::new(BufferStore::new(conn, buffers_dir.clone())),
        config_store: ConfigStore::new(writ_dir.join("config.toml")),
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

/// Opens `name` holding `content` through the command layer, the way the
/// frontend reaches a file, and returns its note id and path.
fn open_note(
    state: &AppState,
    dir: &TempDir,
    name: &str,
    content: &str,
) -> (String, std::path::PathBuf) {
    open_note_at(state, &dir.path().join(name), content)
}

/// [`open_note`] for a path that is not directly under the temp dir.
fn open_note_at(
    state: &AppState,
    path: &std::path::Path,
    content: &str,
) -> (String, std::path::PathBuf) {
    std::fs::write(path, content).expect("write");
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(state, &canonical).expect("open");
    (opened.doc.id, std::path::PathBuf::from(canonical))
}

fn conflict_copies(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains("(conflict "))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn save_updates_the_recorded_disk_state_so_the_next_save_proceeds() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (id, path) = open_note(&state, &dir, "running.md", "first");

    save_buffer_content_inner(&state, &id, "second").expect("first save");
    assert_eq!(
        state.disk_state(&id).expect("recorded").hash,
        sha256_bytes(b"second"),
        "a save records what it just wrote"
    );

    save_buffer_content_inner(&state, &id, "third").expect("a second save is not a conflict");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "third");
}

#[test]
fn a_save_over_a_file_changed_outside_writ_is_stopped_and_the_text_lands_beside_it() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (id, path) = open_note(&state, &dir, "shared.md", "what Writ read");

    std::fs::write(&path, "what another program wrote").unwrap();

    let refused = save_buffer_content_inner(&state, &id, "what the user typed")
        .expect_err("the save must not land");
    assert!(
        refused.starts_with(ERR_FILE_CHANGED_ON_DISK),
        "the frontend reads the code, not the wording: {refused}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "what another program wrote",
        "the change on disk survives"
    );

    let copies = conflict_copies(dir.path());
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(
        std::fs::read_to_string(&copies[0]).unwrap(),
        "what the user typed",
        "the text the save was carrying is beside the note"
    );

    // Reading the file is how the tab catches up, and it moves the record.
    let reloaded = read_buffer_content_inner(&state, &id).expect("read");
    assert_eq!(reloaded, b"what another program wrote");

    save_buffer_content_inner(&state, &id, "what the user typed next").expect("save after reading");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "what the user typed next"
    );
    assert_eq!(
        conflict_copies(dir.path()).len(),
        1,
        "a save that lands writes no copy"
    );
}

#[test]
fn a_stopped_save_comes_back_under_a_stable_code() {
    let changed = writ_storage::errors::StorageError::SourceChangedOnDisk {
        path: "/notes/shared.md".to_string(),
        disk_hash: "deadbeef".to_string(),
        conflict_copy: Some("/notes/shared (conflict 2026-08-29 09.41.07).md".to_string()),
    };
    let message = save_failure_message(&changed);
    assert!(message.starts_with(ERR_FILE_CHANGED_ON_DISK), "{message}");
    assert!(message.contains("/notes/shared.md"), "{message}");

    let waiting = writ_storage::errors::StorageError::SourceNotDownloaded {
        path: "/notes/evicted.md".to_string(),
    };
    let message = save_failure_message(&waiting);
    assert!(message.starts_with(ERR_FILE_NOT_DOWNLOADED), "{message}");
}

#[test]
fn a_refused_destination_comes_back_under_a_stable_code() {
    let linked = writ_storage::errors::StorageError::HardLinkedDestination {
        path: "/notes/linked.md".to_string(),
        links: 2,
    };
    let message = save_failure_message(&linked);
    assert!(message.starts_with(ERR_HARD_LINKED), "{message}");
    assert!(message.contains("/notes/linked.md"), "{message}");

    let locked = writ_storage::errors::StorageError::DestinationReadOnly {
        path: "/notes/locked.md".to_string(),
    };
    let message = save_failure_message(&locked);
    assert!(message.starts_with(ERR_READ_ONLY_DESTINATION), "{message}");
    assert!(message.contains("/notes/locked.md"), "{message}");
}

#[test]
fn a_save_stopped_by_another_program_says_so_rather_than_denying_permission() {
    // What Windows hands back once the retries in writ_storage::atomic run
    // out: the rename was refused because somebody else has the file open,
    // and "you do not have permission to change this file" would send the
    // person looking at the wrong thing.
    let busy = writ_storage::errors::StorageError::Io(std::io::Error::new(
        std::io::ErrorKind::ResourceBusy,
        std::io::Error::from_raw_os_error(5),
    ));
    let message = save_failure_message(&busy);
    assert!(message.starts_with(ERR_FILE_IN_USE), "{message}");
}

#[test]
fn every_other_failure_comes_back_under_the_catch_all_code() {
    let other = writ_storage::errors::StorageError::Consistency {
        message: "note x has no file to save into".to_string(),
    };
    let message = save_failure_message(&other);
    // The editor writes its sentence from the code, so a failure carrying none
    // would reach a person as whatever the layer underneath happened to say.
    assert_eq!(message, format!("{ERR_WRITE_FAILED}: {other}"));
}

#[test]
fn the_copy_a_stopped_save_writes_is_not_an_arrival_in_the_watched_folder() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let watched = TempDir::new().unwrap();
    let root = writ_tauri_lib::security::canonicalize_root(watched.path()).expect("canonical");

    // The folder Writ writes into is the one the inbox watcher is watching,
    // which is the case that turns Writ's own write into an arrival: a toast,
    // a reopened tab, and the window pulled forward.
    let (id, path) = open_note(&state, &watched, "shared.md", "what Writ read");
    let preexisting: std::collections::HashSet<std::path::PathBuf> =
        std::iter::once(path.clone()).collect();

    std::fs::write(&path, "what another program wrote").unwrap();
    save_buffer_content_inner(&state, &id, "what the user typed")
        .expect_err("the save must not land");

    // Listed from the canonical root, not from the temp dir's own path: on
    // macOS the two differ (/var against /private/var) and a path that is not
    // under the root fails the containment check before the ignore set is ever
    // consulted, which would make this test pass for the wrong reason.
    let copies = conflict_copies(&root);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(copies[0].starts_with(&root), "{:?}", copies[0]);

    let arrival = writ_tauri_lib::watcher::handler::classify_inbox_event(
        &copies[0],
        &root,
        &preexisting,
        &state.watcher_ignore,
        writ_core::watcher::ignore::DEFAULT_IGNORE_TTL,
        std::time::Instant::now(),
    );
    assert!(
        arrival.is_none(),
        "Writ's own copy must not come back as somebody else's file: {arrival:?}"
    );
}

#[test]
fn a_file_another_program_drops_in_the_watched_folder_still_arrives() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let watched = TempDir::new().unwrap();
    let root = writ_tauri_lib::security::canonicalize_root(watched.path()).expect("canonical");
    let preexisting: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();

    let dropped = root.join("from-somewhere-else.md");
    std::fs::write(&dropped, "# not ours").unwrap();

    let arrival = writ_tauri_lib::watcher::handler::classify_inbox_event(
        &dropped,
        &root,
        &preexisting,
        &state.watcher_ignore,
        writ_core::watcher::ignore::DEFAULT_IGNORE_TTL,
        std::time::Instant::now(),
    );
    assert!(
        arrival.is_some(),
        "the stamp must not swallow a real arrival"
    );
}

#[test]
fn a_conflict_copy_in_one_folder_does_not_suppress_an_arrival_of_the_same_name_in_another() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let watched = TempDir::new().unwrap();
    let root = writ_tauri_lib::security::canonicalize_root(watched.path()).expect("canonical");
    std::fs::create_dir_all(root.join("a")).unwrap();
    std::fs::create_dir_all(root.join("b")).unwrap();

    let (id, path) = open_note_at(&state, &root.join("a").join("shared.md"), "what Writ read");
    std::fs::write(&path, "what another program wrote").unwrap();
    save_buffer_content_inner(&state, &id, "what the user typed").expect_err("the save is stopped");

    let copies = conflict_copies(&root.join("a"));
    assert_eq!(copies.len(), 1, "{copies:?}");
    let name = copies[0].file_name().expect("name").to_owned();

    // Same name, same bytes, different folder: nothing but the key separates
    // somebody else's file from the copy Writ just wrote.
    let elsewhere = root.join("b").join(&name);
    std::fs::write(&elsewhere, "what the user typed").unwrap();

    let arrival = writ_tauri_lib::watcher::handler::classify_inbox_event(
        &elsewhere,
        &root,
        &std::collections::HashSet::new(),
        &state.watcher_ignore,
        writ_core::watcher::ignore::DEFAULT_IGNORE_TTL,
        std::time::Instant::now(),
    );
    assert!(
        arrival.is_some(),
        "a copy written into a/ must not swallow b/{}",
        name.to_string_lossy()
    );
}

#[test]
fn text_a_save_could_not_write_is_held_for_the_shutdown_snapshot() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    state.record_unsaved_on_exit("note-1", "first pass".to_string());
    // Both ways out hand the same note over, and the later one carries the
    // newer text.
    state.record_unsaved_on_exit("note-1", "what the person last typed".to_string());
    state.record_unsaved_on_exit("note-2", "another note".to_string());

    let held = state.take_unsaved_on_exit();
    assert_eq!(
        held.get("note-1").map(String::as_str),
        Some("what the person last typed")
    );
    assert_eq!(held.get("note-2").map(String::as_str), Some("another note"));

    // Taken once: a second snapshot pass must not write the text again over a
    // file that has since been saved.
    assert!(state.take_unsaved_on_exit().is_empty());
}
