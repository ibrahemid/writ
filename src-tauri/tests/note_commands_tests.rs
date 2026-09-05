//! The note commands at the layer the frontend reaches (ADR-028 §3).

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::hash::sha256_bytes;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_core::watcher::change_event::ExternalChange;
use writ_core::watcher::pending::hold_window;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::{
    read_buffer_content_inner, restore_note_file_inner, save_buffer_content_inner,
    ERR_FILE_CHANGED_ON_DISK, ERR_FILE_MISSING, ERR_FILE_REMOVED_ON_DISK,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::notes::{
    delete_note_inner, move_notes_folder_to, new_note_inner, note_path_for_id, notes_root_text,
    path_is_inside_notes, rename_note_inner, rename_note_recording, save_note_copy_inner,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::{
    create_ignore_set, start_notes_watcher, NOTES_DEBOUNCE_WINDOW,
};
use writ_tauri_lib::watcher::moves::{FileTracking, MoveOutcome};
use writ_tauri_lib::watcher::open_files::start_open_file_watcher;

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
        removal_holds: Default::default(),
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

/// The file a note lives in, as the row records it.
fn note_file(state: &AppState, id: &str) -> std::path::PathBuf {
    let store = state.store.lock().expect("lock");
    let doc = store.get(id).expect("row");
    std::path::PathBuf::from(doc.source_path.expect("the note has no file"))
}

fn title_of(state: &AppState, id: &str) -> String {
    let store = state.store.lock().expect("lock");
    store.get(id).expect("row").title
}

/// Opens a file from outside the notes folder the way the frontend does.
fn open_note_at(state: &AppState, path: &std::path::Path, content: &str) -> String {
    std::fs::write(path, content).expect("write");
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(state, &canonical).expect("open").doc.id
}

#[test]
fn new_note_produces_a_file_on_disk_before_the_app_quits() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);

    let doc = new_note_inner(&state).expect("new note");

    let path = std::path::PathBuf::from(doc.source_path.clone().expect("the note has no file"));
    assert!(
        path.exists(),
        "{} was not created; nothing was written until quit",
        path.display()
    );
    assert!(path.starts_with(state.notes_root()), "{}", path.display());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "");
    assert_eq!(doc.title, path.file_name().unwrap().to_string_lossy());

    // A second one names itself around the first rather than over it.
    let second = new_note_inner(&state).expect("new note");
    let second_path =
        std::path::PathBuf::from(second.source_path.clone().expect("the note has no file"));
    assert_ne!(second_path, path);
    assert!(second_path.exists());
    assert!(path.exists(), "the first note was written over");
}

#[test]
fn rename_note_keeps_the_buffer_id_so_the_tab_keeps_its_content() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "the text").expect("save");
    let before = note_file(&state, &doc.id);

    let renamed = rename_note_inner(&state, &doc.id, "Grocery list").expect("rename");

    assert_eq!(renamed.id, doc.id, "the note's id moved with the rename");
    let after = note_file(&state, &doc.id);
    assert_eq!(after, state.notes_root().join("Grocery list.md"));
    assert!(!before.exists(), "the old name is still there");
    assert_eq!(std::fs::read_to_string(&after).expect("read"), "the text");
    assert_eq!(title_of(&state, &doc.id), "Grocery list.md");

    // The next save lands on the new file rather than recreating the old one.
    save_buffer_content_inner(&state, &doc.id, "more text").expect("save");
    assert_eq!(std::fs::read_to_string(&after).expect("read"), "more text");
    assert!(!before.exists());
}

#[test]
fn rename_keeps_the_extension_a_typed_name_already_carries() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");

    rename_note_inner(&state, &doc.id, "Grocery list.md").expect("rename");

    assert_eq!(
        note_file(&state, &doc.id),
        state.notes_root().join("Grocery list.md")
    );
}

#[test]
fn rename_to_a_name_already_in_the_folder_says_which_one() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");
    std::fs::write(
        state.notes_root().join("Grocery list.md"),
        "somebody else's",
    )
    .expect("seed");
    let before = note_file(&state, &doc.id);

    let error = rename_note_inner(&state, &doc.id, "Grocery list").expect_err("collision");

    assert_eq!(error, "A note named \"Grocery list.md\" is already there.");
    assert!(before.exists(), "the note was renamed anyway");
    assert_eq!(
        std::fs::read_to_string(state.notes_root().join("Grocery list.md")).expect("read"),
        "somebody else's"
    );
}

#[test]
fn rename_to_a_name_with_nothing_in_it_is_stopped() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");
    let before = note_file(&state, &doc.id);

    assert_eq!(
        rename_note_inner(&state, &doc.id, "   ").expect_err("empty"),
        "That name is empty."
    );
    assert_eq!(
        rename_note_inner(&state, &doc.id, "///").expect_err("empty"),
        "That name is empty."
    );
    assert!(before.exists());
}

#[test]
fn rename_refuses_when_the_file_changed_on_disk() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "what Writ read").expect("save");
    let before = note_file(&state, &doc.id);
    std::fs::write(&before, "what somebody else wrote").expect("outside write");

    let error = rename_note_inner(&state, &doc.id, "Grocery list").expect_err("changed");

    assert!(
        error.starts_with(ERR_FILE_CHANGED_ON_DISK),
        "the editor cannot tell what happened from {error:?}"
    );
    assert!(before.exists(), "the file moved out from under the change");
    assert_eq!(
        std::fs::read_to_string(&before).expect("read"),
        "what somebody else wrote"
    );
    assert!(!state.notes_root().join("Grocery list.md").exists());
    assert_eq!(note_file(&state, &doc.id), before);
}

#[test]
fn delete_moves_to_trash_and_closes_the_tab() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "the text").expect("save");
    let path = note_file(&state, &doc.id);
    assert!(path.exists());

    delete_note_inner(&state, &doc.id).expect("delete");

    // Where the note went is the platform's business; that it left its path
    // and left Writ is this test's.
    assert!(!path.exists(), "the note is still at its path");
    let store = state.store.lock().expect("lock");
    assert!(store.get(&doc.id).is_err(), "the row outlived the note");
    drop(store);
    assert!(state.disk_state(&doc.id).is_none());
}

#[test]
fn save_copy_leaves_the_original_untouched() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let elsewhere = dir.path().join("report.md");
    let id = open_note_at(&state, &elsewhere, "the text");

    let copy = save_note_copy_inner(&state, &id, "the text plus more").expect("copy");
    let copy = std::path::PathBuf::from(copy);

    assert_eq!(copy, state.notes_root().join("report.md"));
    assert_eq!(
        std::fs::read_to_string(&copy).expect("read"),
        "the text plus more"
    );
    assert!(elsewhere.exists(), "the file the copy came from is gone");
    assert_eq!(
        std::fs::read_to_string(&elsewhere).expect("read"),
        "the text"
    );
    assert_eq!(
        note_file(&state, &id),
        writ_tauri_lib::security::canonicalize_root(&elsewhere).expect("canonical"),
        "the note followed its copy instead of staying put"
    );

    // The copy is a file inside the notes folder, so it opens with no further
    // permission and is a note of its own.
    let opened = open_file_from_path(&state, copy.to_str().expect("utf-8")).expect("open");
    assert_ne!(opened.doc.id, id);
    assert_eq!(
        sha256_bytes(std::fs::read(&copy).expect("read").as_slice()),
        sha256_bytes(b"the text plus more")
    );
}

#[test]
fn the_notes_folder_is_reported_as_the_path_notes_are_written_to() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");

    let root = notes_root_text(&state);

    assert_eq!(root, state.notes_root().to_string_lossy());
    assert!(
        doc.source_path.as_deref().expect("file").starts_with(&root),
        "a note landed outside the folder the sidebar is told about"
    );
}

#[test]
fn a_note_is_shown_by_the_file_its_row_names() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");

    assert_eq!(
        note_path_for_id(&state, &doc.id).expect("path"),
        doc.source_path.expect("file")
    );
    assert!(note_path_for_id(&state, "no-such-note").is_err());
}

#[test]
fn only_a_file_the_notes_folder_holds_is_inside_it() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let inside = state.notes_root().join("2026-08-29.md");
    std::fs::write(&inside, "the text").expect("seed");
    let outside = dir.path().join("elsewhere.md");
    std::fs::write(&outside, "somebody else's").expect("seed");

    assert!(path_is_inside_notes(
        &state,
        inside.to_str().expect("utf-8")
    ));
    assert!(!path_is_inside_notes(
        &state,
        outside.to_str().expect("utf-8")
    ));
    // A walk back out of the folder is not inside it.
    assert!(!path_is_inside_notes(
        &state,
        state
            .notes_root()
            .join("../elsewhere.md")
            .to_str()
            .expect("utf-8")
    ));
    assert!(!path_is_inside_notes(&state, "elsewhere.md"));
}

#[test]
fn a_file_opened_from_elsewhere_is_never_moved_to_the_trash() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let elsewhere = dir.path().join("somebody-elses.md");
    let id = open_note_at(&state, &elsewhere, "not mine to delete");

    let error = delete_note_inner(&state, &id).expect_err("outside the notes folder");

    assert_eq!(
        error,
        "Only notes in your notes folder can be moved to the Trash from here."
    );
    assert!(elsewhere.exists(), "somebody else's file was deleted");
    assert_eq!(
        std::fs::read_to_string(&elsewhere).expect("read"),
        "not mine to delete"
    );
    let store = state.store.lock().expect("lock");
    assert!(store.get(&id).is_ok(), "the tab was closed anyway");
}

#[test]
fn a_note_that_never_reached_a_file_only_loses_its_row() {
    // Containment has nothing to decide for a note with no file, and there is
    // nothing to hand the Trash. The row still goes.
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let id = {
        let store = state.store.lock().expect("lock");
        let mut mgr = writ_core::buffer::manager::BufferManager::new();
        let doc = mgr.create_buffer(None).expect("mint");
        store.insert(&doc).expect("insert");
        doc.id
    };

    delete_note_inner(&state, &id).expect("delete");

    let store = state.store.lock().expect("lock");
    assert!(store.get(&id).is_err(), "the row outlived the note");
}

#[test]
fn a_rename_the_row_cannot_follow_puts_the_file_back() {
    // The file moves before the row does. If the row write fails in that
    // window, leaving the file moved would leave a row pointing at nothing,
    // which is a note nobody can open again.
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "the text").expect("save");
    let before = note_file(&state, &doc.id);

    let refuse = |_: &writ_storage::buffer_store::BufferStore,
                  _: &str,
                  _: &str,
                  _: &str|
     -> Result<(), StorageError> {
        Err(StorageError::Consistency {
            message: "the row could not be written".to_string(),
        })
    };

    let error =
        rename_note_recording(&state, &doc.id, "Grocery list", &refuse).expect_err("row failed");

    assert!(error.contains("the row could not be written"), "{error}");
    assert!(before.exists(), "the note is not back under its own name");
    assert_eq!(
        std::fs::read_to_string(&before).expect("read"),
        "the text",
        "the note came back without its text"
    );
    assert!(
        !state.notes_root().join("Grocery list.md").exists(),
        "the file was left under the name the row never took"
    );
    assert_eq!(note_file(&state, &doc.id), before);

    // The tab is still usable: the next save lands on the file the row names.
    save_buffer_content_inner(&state, &doc.id, "more text").expect("save");
    assert_eq!(std::fs::read_to_string(&before).expect("read"), "more text");
}

/// Long enough for the 500 ms debounce plus the platform's own notification
/// latency, and nowhere near the ignore TTL.
const SETTLE: Duration = Duration::from_secs(3);

/// Long enough for a write Writ made to be delivered in a batch of its own,
/// so the change a test makes next is judged on its own content.
const APART: Duration = Duration::from_millis(700);

/// A state with both watchers running, as the app runs them: the open-file
/// registry first, because it is how the notes watcher answers "which tab
/// holds this file", then the notes watcher over the notes folder.
///
/// The handles are held by the state, so they live as long as it does.
fn watching_state(dir: &TempDir) -> (Arc<AppState>, mpsc::Receiver<WritEvent>) {
    let state = Arc::new(make_state(dir));
    let (tx, rx) = mpsc::channel();
    state.event_bus.subscribe(move |event| {
        let _ = tx.send(event.clone());
    });

    // The real tracking, reached through the state rather than through an
    // application: a delete here is decided exactly as it is in the app, and
    // the row and the marks it moves are this state's.
    *state.file_tracking.lock().expect("tracking slot") = Some(FileTracking::of_state(&state));

    let open_files = start_open_file_watcher(
        state.event_bus.clone(),
        state.watcher_ignore.clone(),
        &state.notes_root(),
        state.file_tracking(),
    )
    .expect("start the open file watcher");
    *state.open_file_watcher.lock().expect("watcher slot") = Some(open_files);

    let notes = start_notes_watcher(
        state.event_bus.clone(),
        state.notes_root(),
        state.watcher_ignore.clone(),
        state.open_notes(),
        state.file_tracking(),
    )
    .expect("start the notes watcher");
    *state.notes_watcher.lock().expect("notes watcher slot") = Some(notes);

    (state, rx)
}

/// Writes `bytes` the way another program does: a sibling temp file renamed
/// over the target, which gives the file a new inode.
fn rewrite_from_outside(path: &std::path::Path, bytes: &[u8]) {
    let temp = path.with_extension("other-program-tmp");
    std::fs::write(&temp, bytes).expect("write temp");
    std::fs::rename(&temp, path).expect("rename over target");
}

/// Every `BufferExternal` the bus carried within `SETTLE`.
fn external_events(rx: &mpsc::Receiver<WritEvent>) -> Vec<WritEvent> {
    let deadline = Instant::now() + SETTLE;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(event @ WritEvent::BufferExternal { .. }) => seen.push(event),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    seen
}

/// The buffer id and path one `BufferExternal` names.
fn named_by(event: &WritEvent) -> (&str, &str) {
    match event {
        WritEvent::BufferExternal {
            buffer_id, path, ..
        } => (buffer_id.as_str(), path.as_str()),
        other => panic!("not an external change: {other:?}"),
    }
}

#[test]
fn a_note_created_this_session_hears_that_another_program_rewrote_it() {
    // Cmd+N is how nearly every note comes into existence, and its file is in
    // the notes folder, which the notes watcher covers. Covering the folder is
    // not enough: the watcher has to be able to say which tab a changed path
    // belongs to, and only the registry answers that. A created note that
    // never reached the registry went on showing text its file no longer held
    // until a save was refused by the write guard.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    rewrite_from_outside(&path, b"rewritten by another program\n");

    let seen = external_events(&rx);
    assert_eq!(seen.len(), 1, "the tab must be told once, saw {seen:?}");
    let (buffer_id, named) = named_by(&seen[0]);
    assert_eq!(buffer_id, doc.id);
    assert_eq!(std::path::Path::new(named), path);
}

#[test]
fn a_note_renamed_this_session_hears_about_its_new_file_and_not_its_old_name() {
    // The rename moves the file and re-keys the row. A registry left holding
    // the old path answers for the wrong file twice over: changes to the
    // renamed file reach no tab, and a later note taking the freed name has
    // them delivered to this one.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    let before = note_file(&state, &doc.id);
    rename_note_inner(&state, &doc.id, "Grocery list").expect("rename");
    let after = note_file(&state, &doc.id);
    assert_ne!(after, before);
    std::thread::sleep(APART);

    // Something else takes the name the rename freed, and the renamed file is
    // rewritten. Only the second is this tab's.
    std::fs::write(&before, b"a different note under the old name\n").expect("write old name");
    rewrite_from_outside(&after, b"rewritten by another program\n");

    let seen = external_events(&rx);
    assert_eq!(seen.len(), 1, "the tab must be told once, saw {seen:?}");
    let (buffer_id, named) = named_by(&seen[0]);
    assert_eq!(buffer_id, doc.id);
    assert_eq!(
        std::path::Path::new(named),
        after,
        "the tab was told about the name it no longer has"
    );
}

#[test]
fn a_note_given_its_file_on_first_save_is_followed_from_that_save() {
    // A note with no file yet gets one on its first save, which is the other
    // way a tab comes to hold a file this session.
    let dir = TempDir::new().expect("temp dir");
    let (state, _rx) = watching_state(&dir);

    let doc = {
        let store = state.store.lock().expect("lock");
        let doc = writ_core::buffer::manager::BufferManager::new()
            .create_buffer(Some("Fileless".to_string()))
            .expect("create");
        store.insert(&doc).expect("insert");
        doc
    };
    assert_eq!(state.open_notes().note_at(&state.notes_root()), None);

    save_buffer_content_inner(&state, &doc.id, "first text").expect("save");

    let path = note_file(&state, &doc.id);
    assert_eq!(
        state.open_notes().note_at(&path).as_deref(),
        Some(doc.id.as_str()),
        "the file the save attached is not followed"
    );
}

#[test]
fn open_tabs_follow_their_files_when_the_notes_folder_moves() {
    let dir = TempDir::new().expect("temp dir");
    let (state, _rx) = watching_state(&dir);
    let doc = new_note_inner(&state).expect("new note");
    let before = note_file(&state, &doc.id);

    let elsewhere = TempDir::new().expect("somewhere else");
    let destination = elsewhere.path().join("Moved");
    move_notes_folder_to(&state, &destination).expect("move the notes folder");

    let after = note_file(&state, &doc.id);
    assert_ne!(after, before);
    assert_eq!(
        state.open_notes().note_at(&after).as_deref(),
        Some(doc.id.as_str()),
        "the tab is still following the folder Writ left"
    );
    assert_eq!(
        state.open_notes().note_at(&before),
        None,
        "the tab still answers for a file that is not there"
    );
}

#[test]
fn a_file_opened_from_outside_the_notes_folder_survives_the_move() {
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let workspace = TempDir::new().expect("a folder Writ does not own");
    let outside = workspace.path().join("outside.md");
    let id = open_note_at(&state, &outside, "opened from elsewhere");

    let elsewhere = TempDir::new().expect("somewhere else");
    move_notes_folder_to(&state, &elsewhere.path().join("Moved")).expect("move the notes folder");

    rewrite_from_outside(&outside, b"another program got there first");

    let events = external_events(&rx);
    assert!(
        events.iter().any(|event| named_by(event).0 == id),
        "the move took a watch on a folder that has nothing to do with it: {events:?}"
    );
}

/// The change one `BufferExternal` reports, and where it says the file went.
fn change_of(event: &WritEvent) -> (&ExternalChange, Option<&str>) {
    match event {
        WritEvent::BufferExternal {
            change, new_path, ..
        } => (change, new_path.as_deref()),
        other => panic!("not an external change: {other:?}"),
    }
}

#[test]
fn a_note_moved_inside_the_notes_folder_keeps_its_tab_and_leaves_nothing_behind() {
    // The headline of W4. Moving a note in Finder must not make the tab a
    // stale window onto a path nothing holds: the next autosave would put the
    // file back where it was, and in a synced folder every device would get
    // the duplicate.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    let sub = state.notes_root().join("archive");
    std::fs::create_dir_all(&sub).expect("subfolder");
    let after = sub.join("moved-by-finder.md");
    std::thread::sleep(APART);

    std::fs::rename(&before, &after).expect("move the file the way Finder does");

    let seen = external_events(&rx);
    assert_eq!(
        seen.len(),
        1,
        "exactly one path update, and no other news, saw {seen:?}"
    );
    let (buffer_id, named) = named_by(&seen[0]);
    assert_eq!(buffer_id, doc.id);
    assert_eq!(
        std::path::Path::new(named),
        before,
        "the message names the path the tab knows, so the tab can recognise it"
    );
    let (change, new_path) = change_of(&seen[0]);
    assert_eq!(change, &ExternalChange::Moved);
    assert_eq!(new_path.map(std::path::Path::new), Some(after.as_path()));

    assert_eq!(
        note_file(&state, &doc.id),
        after,
        "the row still points at the path the file left"
    );
    assert_eq!(
        title_of(&state, &doc.id),
        "moved-by-finder.md",
        "the tab keeps the name the file had before it moved"
    );

    save_buffer_content_inner(&state, &doc.id, "text worth keeping, edited").expect("save again");
    assert!(
        !before.exists(),
        "the save recreated the note at the path the user moved it away from"
    );
    assert_eq!(
        std::fs::read_to_string(&after).expect("read the moved file"),
        "text worth keeping, edited"
    );
}

#[test]
fn a_note_deleted_outside_writ_is_not_recreated_by_the_next_save() {
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("delete the note the way Finder does");

    let seen = external_events(&rx);
    assert_eq!(seen.len(), 1, "the tab must be told once, saw {seen:?}");
    assert_eq!(named_by(&seen[0]).0, doc.id);
    assert_eq!(change_of(&seen[0]), (&ExternalChange::Removed, None));

    let refused = save_buffer_content_inner(&state, &doc.id, "text worth keeping, edited")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the refusal has to carry its own code, got {refused}"
    );
    assert!(
        !path.exists(),
        "the save put back the file the user threw away"
    );
}

#[test]
fn a_note_deleted_outside_writ_goes_back_to_its_path_when_asked_for() {
    // The refusal above is about a keystroke. This is the person asking, and
    // the text in the tab is the last copy of the note (ADR-028 §1), so the
    // one thing that must not happen is Writ having nowhere to put it.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("delete the note the way Finder does");
    let seen = external_events(&rx);
    assert_eq!(change_of(&seen[0]), (&ExternalChange::Removed, None));

    restore_note_file_inner(&state, &doc.id, "text worth keeping, edited")
        .expect("the restore must land");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the file back"),
        "text worth keeping, edited"
    );

    // The record follows the file, so the next keystroke saves the ordinary
    // way rather than being refused over a file that is there.
    save_buffer_content_inner(&state, &doc.id, "and edited again").expect("the next save");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the file back"),
        "and edited again"
    );
}

#[test]
fn a_restore_with_nowhere_to_write_says_so_and_leaves_the_note_removed() {
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("delete the note the way Finder does");
    let seen = external_events(&rx);
    assert_eq!(change_of(&seen[0]), (&ExternalChange::Removed, None));

    let folder = path.parent().expect("the note's folder").to_path_buf();
    std::fs::remove_dir_all(&folder).expect("take the folder away too");

    let refused = restore_note_file_inner(&state, &doc.id, "text worth keeping, edited")
        .expect_err("a write with nowhere to land must be refused");
    assert!(
        refused.starts_with(ERR_FILE_MISSING),
        "the refusal has to carry its own code, got {refused}"
    );
    // A restore that did not land leaves the note removed, so nothing later
    // recreates the file quietly.
    let after = save_buffer_content_inner(&state, &doc.id, "and edited again")
        .expect_err("the next save must still be refused");
    assert!(
        after.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the note stopped being marked removed, got {after}"
    );
}

#[test]
fn a_sync_client_replacing_a_file_is_an_external_modification_and_not_a_move() {
    // Delete plus create at the same path, which is how more than one sync
    // client lands an update. The file is a different file, and the tab is
    // told what every other rewrite tells it.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "as writ left it").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("remove");
    std::fs::write(&path, b"as the sync client left it\n").expect("create in its place");

    let seen = external_events(&rx);
    assert!(!seen.is_empty(), "the tab heard nothing about the rewrite");
    for event in &seen {
        assert_eq!(named_by(event).0, doc.id);
        assert_eq!(
            change_of(event).0,
            &ExternalChange::Modified,
            "a replaced file is a modification, not a move and not a delete: {event:?}"
        );
    }
    assert!(
        !state.is_removed_on_disk(&doc.id),
        "the tab must still be writing to the file that took the path"
    );
    // And W2 governs from here, which is the whole of what the verdict
    // decides: the write guard stops the save, writes the text beside the note
    // and says why, rather than the tab refusing to write at all.
    let refused = save_buffer_content_inner(&state, &doc.id, "edited after the sync landed")
        .expect_err("a file rewritten under writ is the write guard's business");
    assert!(
        refused.starts_with(ERR_FILE_CHANGED_ON_DISK),
        "got {refused}"
    );
}

#[test]
fn a_note_put_back_from_the_trash_re_attaches_to_its_file() {
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    let trash = dir.path().join("trash-stand-in.md");
    std::thread::sleep(APART);

    std::fs::rename(&path, &trash).expect("move to the trash");
    let removal = external_events(&rx);
    assert_eq!(
        removal.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Removed],
        "saw {removal:?}"
    );
    assert!(state.is_removed_on_disk(&doc.id));

    std::fs::rename(&trash, &path).expect("put it back");
    let restored = external_events(&rx);
    assert!(
        restored
            .iter()
            .any(|event| change_of(event).0 == &ExternalChange::Modified),
        "a file that came back must reach its tab, saw {restored:?}"
    );
    assert!(
        !state.is_removed_on_disk(&doc.id),
        "the tab is still refusing to write to a file that is there"
    );
    save_buffer_content_inner(&state, &doc.id, "edited after the restore").expect("save");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "edited after the restore"
    );
}

#[test]
fn a_tab_restored_at_launch_onto_a_file_that_is_gone_writes_nothing() {
    // Nothing about a deleted file survives a relaunch, so the state is read
    // back from the file when the tab is restored. Without that, a note whose
    // file was deleted while Writ was closed comes back looking ordinary and
    // recreates the file on its first save.
    let dir = TempDir::new().expect("temp dir");
    let (state, _rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::fs::remove_file(&path).expect("delete while writ is not looking");
    state.forget_source_record(&doc.id);

    let row = {
        let store = state.store.lock().expect("lock");
        store.get(&doc.id).expect("row")
    };
    state.follow_note_file(&row);

    let refused = save_buffer_content_inner(&state, &doc.id, "edited after the relaunch")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "got {refused}"
    );
    assert!(!path.exists());
}

#[test]
fn a_file_opened_from_outside_the_notes_folder_follows_its_move_too() {
    // The other watcher, and the case the notes watcher cannot see: a file in
    // a folder Writ follows only because a tab holds it.
    let dir = TempDir::new().expect("temp dir");
    let elsewhere = TempDir::new().expect("some other folder");
    let (state, rx) = watching_state(&dir);

    let file = elsewhere.path().join("shared.md");
    std::fs::write(&file, b"as another program left it\n").expect("seed");
    let canonical = canonicalize_for_authorization(&file).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");
    let renamed = elsewhere.path().join("renamed-by-somebody.md");
    std::thread::sleep(APART);

    std::fs::rename(&file, &renamed).expect("rename");

    let seen = external_events(&rx);
    assert_eq!(seen.len(), 1, "the tab must be told once, saw {seen:?}");
    assert_eq!(named_by(&seen[0]).0, opened.doc.id);
    let (change, new_path) = change_of(&seen[0]);
    assert_eq!(change, &ExternalChange::Moved);
    let landed = new_path.expect("a move names where the file went");
    assert_eq!(
        canonicalize_for_authorization(std::path::Path::new(landed)).expect("canonical"),
        canonicalize_for_authorization(&renamed).expect("canonical")
    );
    assert_eq!(
        canonicalize_for_authorization(&note_file(&state, &opened.doc.id)).expect("canonical"),
        canonicalize_for_authorization(&renamed).expect("canonical"),
        "the row still points at the path the file left"
    );
}

#[test]
fn a_rename_after_another_program_rewrote_the_file_is_still_a_move() {
    // Nearly every program that writes a file writes a sibling temp and
    // renames it over the target: vim, VS Code, git checkout, rsync, every
    // sync client. The path is unchanged and the file behind it is a different
    // file. A tab still holding the id it read at open then reads its own next
    // rename as a deletion, marks itself removed and refuses every later save
    // over a file sitting at its new path with the user's text in it.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    rewrite_from_outside(&before, b"rewritten by another program\n");
    let rewrite = external_events(&rx);
    assert_eq!(
        rewrite.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Modified],
        "the rewrite has to reach the tab before the rename, saw {rewrite:?}"
    );
    // The tab reloads, which is what the editor does with that news and what
    // leaves the write guard satisfied. It reads the file; it says nothing
    // about the file's id.
    read_buffer_content_inner(&state, &doc.id).expect("reload");

    let after = state.notes_root().join("renamed-by-finder.md");
    std::fs::rename(&before, &after).expect("rename the way Finder does");

    let seen = external_events(&rx);
    assert_eq!(
        seen.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Moved],
        "a rename after an external rewrite must still be a move, saw {seen:?}"
    );
    assert_eq!(
        change_of(&seen[0]).1.map(std::path::Path::new),
        Some(after.as_path())
    );
    assert!(
        !state.is_removed_on_disk(&doc.id),
        "the tab stopped writing to a file that is there"
    );
    assert_eq!(note_file(&state, &doc.id), after);

    save_buffer_content_inner(&state, &doc.id, "edited after the rename").expect("save");
    assert_eq!(
        std::fs::read_to_string(&after).expect("read the moved file"),
        "edited after the rename"
    );
    assert!(
        !before.exists(),
        "the save recreated the file at the old path"
    );
}

#[test]
fn a_rewrite_and_a_rename_in_one_window_are_a_removal_that_replaces_no_text() {
    // Two writes inside one window are reported as one: a program rewrote the
    // file and renamed it, and the only event is the path going empty. The
    // rewrite is never reported, so the id on record is the one it retired and
    // nothing carries it anywhere. Nothing else names the file either, so the
    // tab is told its file went rather than being pointed at a path on
    // evidence that cannot tell a copy from a rename.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    rewrite_from_outside(&before, b"text worth keeping");
    let after = state.notes_root().join("renamed-by-finder.md");
    std::fs::rename(&before, &after).expect("rename the way Finder does");

    let seen = external_events(&rx);
    assert_eq!(seen.len(), 1, "the tab must be told once, saw {seen:?}");
    assert_eq!(named_by(&seen[0]).0, doc.id);
    assert_eq!(change_of(&seen[0]), (&ExternalChange::Removed, None));
    assert!(state.is_removed_on_disk(&doc.id));
    assert!(
        matches!(
            &seen[0],
            WritEvent::BufferExternal {
                disk_hash: None,
                ..
            }
        ),
        "a removal carrying a digest has the editor replace the text it holds"
    );
    assert_eq!(
        note_file(&state, &doc.id),
        before,
        "the row followed a file the note was never given"
    );

    let refused = save_buffer_content_inner(&state, &doc.id, "edited after the rename")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the refusal has to carry its own code, got {refused}"
    );
    assert_eq!(
        std::fs::read_to_string(&after).expect("read the file at the new path"),
        "text worth keeping",
        "the save wrote into a file the tab was never given"
    );
}

#[test]
fn a_note_dragged_out_of_the_notes_folder_leaves_a_twin_of_its_bytes_alone() {
    // A file that leaves every watched folder carries its id away with it, so
    // nothing in the window carries it and the note is gone as far as Writ can
    // see. A file holding the same bytes in the same window is a copy, a
    // template, or a sync client's conflicted copy as easily as it is the
    // note, and taking it would let the next save replace its content.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    let outside = dir.path().join("outside-every-watched-folder");
    std::fs::create_dir_all(&outside).expect("a folder no watcher covers");
    let twin = state.notes_root().join("aaa-twin.md");
    std::fs::write(&twin, "text worth keeping").expect("the twin");
    let dragged = outside.join("dragged-out.md");
    std::fs::rename(&before, &dragged).expect("drag it out of the notes folder");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(told.len(), 1, "the tab must be told once, saw {seen:?}");
    assert_eq!(change_of(told[0]), (&ExternalChange::Removed, None));
    assert_eq!(
        note_file(&state, &doc.id),
        before,
        "the row followed a file the note was never in"
    );

    let refused = save_buffer_content_inner(&state, &doc.id, "edited after the move")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the refusal has to carry its own code, got {refused}"
    );
    assert_eq!(
        std::fs::read_to_string(&twin).expect("read the twin"),
        "text worth keeping",
        "the save wrote over a file the note was never in"
    );
    assert_eq!(
        std::fs::read_to_string(&dragged).expect("read the file the user dragged out"),
        "text worth keeping"
    );
}

#[test]
fn a_note_moved_away_leaves_a_second_note_with_the_same_bytes_on_its_own_file() {
    // Two notes from one template hold the same bytes, which makes the file at
    // risk as likely to be another note as anything else. Both rows landed on
    // one path: the second note's text was overwritten by the first note's
    // next save while its tab still believed it owned that file.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let one = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &one.id, "text worth keeping").expect("save");
    let one_before = note_file(&state, &one.id);
    let two = new_note_inner(&state).expect("a second note");
    save_buffer_content_inner(&state, &two.id, "text worth keeping").expect("save");
    let two_file = note_file(&state, &two.id);
    std::thread::sleep(APART);

    let outside = dir.path().join("outside-every-watched-folder");
    std::fs::create_dir_all(&outside).expect("a folder no watcher covers");
    rewrite_from_outside(&two_file, b"text worth keeping");
    let one_moved = outside.join("dragged-out.md");
    std::fs::rename(&one_before, &one_moved).expect("drag the first note out");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == one.id).collect();
    assert_eq!(
        told.len(),
        1,
        "the moved note must be told once, saw {seen:?}"
    );
    assert_eq!(change_of(told[0]), (&ExternalChange::Removed, None));
    assert_eq!(note_file(&state, &one.id), one_before);
    assert_eq!(
        note_file(&state, &two.id),
        two_file,
        "the second note's row followed the first note's move"
    );

    let refused = save_buffer_content_inner(&state, &one.id, "edited after the move")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the refusal has to carry its own code, got {refused}"
    );
    assert_eq!(
        std::fs::read_to_string(&two_file).expect("read the second note's file"),
        "text worth keeping",
        "the first note's save wrote over the second note's file"
    );
}

#[test]
fn a_deleted_note_leaves_a_file_holding_the_same_bytes_alone() {
    // No move at all. The note's bytes exist at no path the note was ever at,
    // and a file that happens to hold the same bytes is still somebody else's
    // file: writing the note into it would replace its content with no event
    // and no error.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    let twin = state.notes_root().join("aaa-twin.md");
    std::fs::write(&twin, "text worth keeping").expect("the twin");
    std::fs::remove_file(&path).expect("delete the note the way Finder does");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(told.len(), 1, "the tab must be told once, saw {seen:?}");
    assert_eq!(change_of(told[0]), (&ExternalChange::Removed, None));

    let refused = save_buffer_content_inner(&state, &doc.id, "text worth keeping, edited")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the refusal has to carry its own code, got {refused}"
    );
    assert_eq!(
        std::fs::read_to_string(&twin).expect("read the twin"),
        "text worth keeping",
        "the save wrote over a file the note was never in"
    );
    assert!(
        !path.exists(),
        "the save put back the file the user threw away"
    );
}

#[test]
fn a_note_replaced_by_a_folder_of_the_same_name_reads_as_a_file_that_went() {
    // A path holding a directory holds no note. The event was dropped for
    // being about something that is not a file, so the tab heard nothing, kept
    // the dead file's id, and its next save came back as a raw
    // `Is a directory` instead of saying the file is gone.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("delete the note's file");
    std::fs::create_dir(&path).expect("a folder takes its name");

    let seen = external_events(&rx);
    assert_eq!(
        seen.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Removed],
        "a folder where the file was is a file that went, saw {seen:?}"
    );
    assert!(state.is_removed_on_disk(&doc.id));

    let refused = save_buffer_content_inner(&state, &doc.id, "edited after the folder appeared")
        .expect_err("the save has to refuse");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the tab must say the file is gone rather than pass on an errno, saw {refused}"
    );
    assert!(path.is_dir(), "the save wrote over the folder");
}

#[test]
fn a_file_outside_the_notes_folder_re_attaches_when_it_comes_back() {
    // The notes watcher covers the restore inside the notes folder. A file
    // opened from anywhere else has only the open-file watcher to hear it come
    // back, and that call site is the whole of the case.
    let dir = TempDir::new().expect("temp dir");
    let elsewhere = TempDir::new().expect("some other folder");
    let trash = TempDir::new().expect("somewhere the watcher cannot see");
    let (state, rx) = watching_state(&dir);

    let file = elsewhere.path().join("shared.md");
    std::fs::write(&file, b"as another program left it\n").expect("seed");
    let canonical = canonicalize_for_authorization(&file).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");
    let held = trash.path().join("shared.md");
    std::thread::sleep(APART);

    std::fs::rename(&file, &held).expect("move to the trash");
    let removal = external_events(&rx);
    assert_eq!(
        removal.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Removed],
        "saw {removal:?}"
    );
    assert!(state.is_removed_on_disk(&opened.doc.id));

    std::fs::rename(&held, &file).expect("put it back");
    let restored = external_events(&rx);
    assert!(
        restored
            .iter()
            .any(|event| change_of(event).0 == &ExternalChange::Modified),
        "a file that came back must reach its tab, saw {restored:?}"
    );
    assert!(
        !state.is_removed_on_disk(&opened.doc.id),
        "the tab is still refusing to write to a file that is there"
    );
    read_buffer_content_inner(&state, &opened.doc.id).expect("reload");
    save_buffer_content_inner(&state, &opened.doc.id, "edited after the restore").expect("save");
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        "edited after the restore"
    );
}

#[test]
fn deleting_one_name_of_a_hard_linked_file_follows_the_name_that_is_left() {
    // A hard link is one file with two names. Deleting one of them deletes a
    // name, not the file: the bytes the tab is editing are still there under
    // the other name, in a folder Writ watches. Reporting a removal would
    // refuse every later save over a file that exists (ADR-033 §12).
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    let survivor = state.notes_root().join("second-name.md");
    std::fs::hard_link(&path, &survivor).expect("a second name for the same file");
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("delete one of the two names");

    let seen = external_events(&rx);
    assert_eq!(
        seen.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Moved],
        "the file is still there under its other name, saw {seen:?}"
    );
    assert_eq!(
        change_of(&seen[0]).1.map(std::path::Path::new),
        Some(survivor.as_path())
    );
    assert!(!state.is_removed_on_disk(&doc.id));
    assert_eq!(note_file(&state, &doc.id), survivor);

    save_buffer_content_inner(&state, &doc.id, "edited after the name went").expect("save");
    assert_eq!(
        std::fs::read_to_string(&survivor).expect("read"),
        "edited after the name went"
    );
    assert!(
        !path.exists(),
        "the save recreated the name that was deleted"
    );
}

#[test]
fn one_move_seen_by_both_watchers_moves_the_row_once() {
    // A file inside the notes folder that a tab also holds is reported by both
    // watchers, and each deduplicates only its own batch. The row is what the
    // next save reads its destination from, so the second report has to find
    // the move already applied and say nothing.
    let dir = TempDir::new().expect("temp dir");
    let state = Arc::new(make_state(&dir));

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    let after = state.notes_root().join("moved-by-finder.md");
    std::fs::rename(&before, &after).expect("move the file the way Finder does");

    let files = FileTracking::of_state(&state).files;
    assert_eq!(
        files.note_file_moved(&doc.id, &before, &after),
        MoveOutcome::Followed,
        "the first watcher to see the move applies it"
    );
    assert_eq!(note_file(&state, &doc.id), after);
    assert_eq!(title_of(&state, &doc.id), "moved-by-finder.md");

    assert_eq!(
        files.note_file_moved(&doc.id, &before, &after),
        MoveOutcome::AlreadyThere,
        "the second watcher's copy of the same move is not news"
    );
    assert_eq!(note_file(&state, &doc.id), after);
    assert_eq!(title_of(&state, &doc.id), "moved-by-finder.md");
}

/// A gap wide enough that the two halves of the rename below cannot land in
/// one delivery, and narrow enough that the file is back in the notes folder
/// before the removal it raised is announced.
///
/// A debounce window closes on a deadline set by its first event and never
/// extends it (ADR-033 §5), so a change this long after another is delivered
/// separately. The hold is twice the window, so the second half has one whole
/// window of its own to arrive in after the first was read.
const SPLIT: Duration = Duration::from_millis(900);

#[test]
fn a_rename_whose_halves_land_in_two_windows_is_still_a_move() {
    // What a rename looks like when it straddles a debounce deadline: the
    // notes folder is delivered the path going empty with nothing that could
    // answer for it, and the file only turns up at its new name in a later
    // delivery. Answering the first delivery on its own marks the tab off a
    // file that is sitting one folder away (ADR-033 §14).
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    // Out of the watched folder and back into it, a window apart. A rename
    // keeps the inode, so what comes back is the file the record names.
    let elsewhere = dir.path().join("outside-every-watched-folder");
    std::fs::create_dir_all(&elsewhere).expect("a folder nothing watches");
    let parked = elsewhere.join("in-flight.md");
    std::fs::rename(&before, &parked).expect("the first half");
    std::thread::sleep(SPLIT);
    let after = state.notes_root().join("renamed-across-two-windows.md");
    std::fs::rename(&parked, &after).expect("the second half");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(
        told.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Moved],
        "a rename delivered in two halves is still one move, saw {seen:?}"
    );
    assert_eq!(
        change_of(told[0]).1.map(std::path::Path::new),
        Some(after.as_path())
    );
    assert!(
        !state.is_removed_on_disk(&doc.id),
        "the tab stopped writing to a file that is there"
    );
    assert_eq!(note_file(&state, &doc.id), after);

    save_buffer_content_inner(&state, &doc.id, "edited after the rename").expect("save");
    assert_eq!(
        std::fs::read_to_string(&after).expect("read the moved file"),
        "edited after the rename"
    );
}

#[test]
fn a_removal_nothing_answers_is_announced_when_the_hold_passes() {
    // The other side of the hold: waiting for a second delivery must not cost
    // the deletion itself. Nothing turns up carrying the file's id, so the
    // removal is announced once its window passes, and the tab is marked off
    // its file at that point and not before.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    let deleted_at = Instant::now();
    std::fs::remove_file(&path).expect("delete the note the way Finder does");

    let deadline = Instant::now() + SETTLE;
    let mut told_at = None;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(event @ WritEvent::BufferExternal { .. }) => {
                if named_by(&event).0 == doc.id {
                    told_at.get_or_insert_with(Instant::now);
                }
                seen.push(event);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(told.len(), 1, "the tab must be told once, saw {seen:?}");
    assert_eq!(change_of(told[0]), (&ExternalChange::Removed, None));
    assert!(
        told_at.expect("the removal was announced") - deleted_at
            >= hold_window(NOTES_DEBOUNCE_WINDOW),
        "the removal was announced without waiting out the hold"
    );
    assert!(state.is_removed_on_disk(&doc.id));

    let refused = save_buffer_content_inner(&state, &doc.id, "text worth keeping, edited")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the refusal has to carry its own code, got {refused}"
    );
}

/// Long enough to be past the delivery that starts the hold, and well short of
/// the deadline the hold ends on.
const INSIDE_THE_HOLD: Duration = Duration::from_millis(700);

/// What a save is allowed to take when it lands inside a hold: the hold itself
/// plus the window the announcement is delivered on.
fn the_longest_a_held_save_may_wait() -> Duration {
    hold_window(NOTES_DEBOUNCE_WINDOW) + NOTES_DEBOUNCE_WINDOW
}

/// How far apart the answer that releases a held save and the announcement of
/// the same removal may land. Both come out of one pass over the expired hold.
/// A save the watcher never answers waits out its own deadline instead, a
/// debounce window behind the announcement.
fn released_together() -> Duration {
    NOTES_DEBOUNCE_WINDOW / 2
}

/// The two windows decision 14's consequences bullet quotes by number, read
/// off the constants the watcher runs on. A failure here is the documented
/// numbers having gone stale, not the watcher having gone wrong.
#[test]
fn the_adr_quotes_the_windows_the_notes_watcher_runs_on() {
    assert_eq!(NOTES_DEBOUNCE_WINDOW, Duration::from_millis(500));
    assert_eq!(
        hold_window(NOTES_DEBOUNCE_WINDOW),
        Duration::from_millis(1000)
    );
}

#[test]
fn a_save_inside_the_hold_of_a_deletion_recreates_nothing() {
    // The hold is the window in which the record still says the note has a
    // file, so nothing in it refuses a write on its own. A save landing there
    // would put the deleted file back holding the tab's text, and in a synced
    // folder put it back on every device (ADR-033 §12).
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("delete the note the way Finder does");
    std::thread::sleep(INSIDE_THE_HOLD);

    let refused = save_buffer_content_inner(&state, &doc.id, "written during the hold")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "a save inside the hold is refused the same way as one after the announcement, got {refused}"
    );
    assert!(
        !path.exists(),
        "the file the person deleted is back on disk"
    );

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(
        told.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Removed],
        "the deletion is announced once and the save says nothing, saw {seen:?}"
    );
    assert!(state.is_removed_on_disk(&doc.id));
}

#[test]
fn a_save_inside_the_hold_of_a_split_rename_lands_at_the_new_path() {
    // The variant that costs a file rather than resurrecting one: the save
    // writes a new file at the path the rename emptied, the move is never
    // announced, and the person is left with two files where they renamed one.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    let elsewhere = dir.path().join("outside-every-watched-folder");
    std::fs::create_dir_all(&elsewhere).expect("a folder nothing watches");
    let parked = elsewhere.join("in-flight.md");
    std::fs::rename(&before, &parked).expect("the first half");

    // On its own thread: the save blocks until the hold is answered, and the
    // second half of the rename is what answers it.
    let saver = {
        let state = Arc::clone(&state);
        let id = doc.id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(INSIDE_THE_HOLD);
            save_buffer_content_inner(&state, &id, "written during the hold")
        })
    };
    std::thread::sleep(SPLIT);
    let after = state.notes_root().join("renamed-across-two-windows.md");
    std::fs::rename(&parked, &after).expect("the second half");
    saver.join().expect("the saving thread").expect("the save");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(
        told.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Moved],
        "the rename is still one move with a save inside it, saw {seen:?}"
    );
    assert_eq!(note_file(&state, &doc.id), after);
    assert!(
        !before.exists(),
        "the save put a second file where the rename emptied"
    );
    assert_eq!(
        std::fs::read_to_string(&after).expect("read the moved file"),
        "written during the hold",
        "the text typed during the hold has to reach the file the note is on"
    );
}

#[test]
fn a_stranger_at_the_path_during_the_hold_is_still_a_deletion() {
    // Something else at the path is not the note's file coming back. Reading
    // the path alone cannot tell them apart, and forgetting the hold for a
    // stranger leaves the tab bound to a file it never opened.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    std::fs::remove_file(&path).expect("delete the note the way Finder does");
    std::thread::sleep(INSIDE_THE_HOLD);
    // A new file at the same path: another program, a sync client, a restore.
    std::fs::write(&path, b"somebody else's file").expect("the stranger's write");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert!(
        told.iter()
            .any(|e| change_of(e).0 == &ExternalChange::Removed),
        "the note's own file is gone and nothing said so, saw {seen:?}"
    );
    assert!(
        !told
            .iter()
            .any(|e| change_of(e).0 == &ExternalChange::Moved),
        "the tab followed a file it never opened, saw {seen:?}"
    );
    assert!(state.is_removed_on_disk(&doc.id));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the stranger's file"),
        "somebody else's file",
        "the stranger's file was written over"
    );
}

#[test]
fn the_same_file_back_at_its_path_during_the_hold_lets_the_save_through() {
    // A rename out and straight back: the file the record names is at the path
    // it left, so there was never anything to announce, and the save that was
    // waiting on the hold lands where it always would have.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    let elsewhere = dir.path().join("outside-every-watched-folder");
    std::fs::create_dir_all(&elsewhere).expect("a folder nothing watches");
    let parked = elsewhere.join("in-flight.md");
    std::fs::rename(&path, &parked).expect("out of the folder");

    let saver = {
        let state = Arc::clone(&state);
        let id = doc.id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(INSIDE_THE_HOLD);
            save_buffer_content_inner(&state, &id, "written while the file was away")
        })
    };
    std::thread::sleep(Duration::from_millis(800));
    std::fs::rename(&parked, &path).expect("and back to the same path");
    saver.join().expect("the saving thread").expect("the save");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert!(
        !told
            .iter()
            .any(|e| change_of(e).0 == &ExternalChange::Removed),
        "a file that never left was announced as deleted, saw {seen:?}"
    );
    assert!(!state.is_removed_on_disk(&doc.id));
    assert_eq!(note_file(&state, &doc.id), path);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the note"),
        "written while the file was away"
    );
}

#[test]
fn a_save_inside_a_hold_waits_for_the_answer_and_no_longer() {
    // Three bounds. Below: the save cannot write before the hold is answered,
    // or it writes to a path nobody can classify yet. Above: the wait ends on
    // the hold's own deadline, so a note whose watcher stopped costs a save one
    // window and not a hung tab. Between: the answer that releases the save and
    // the announcement of the removal come out of the same pass over the
    // expired hold, so they land together, and a save that is instead left to
    // time out lands a debounce window behind the announcement.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    let deleted_at = Instant::now();
    std::fs::remove_file(&path).expect("delete the note the way Finder does");

    // On its own thread, so the announcement can be timed while the save is
    // still blocked on the hold.
    let saver = {
        let state = Arc::clone(&state);
        let id = doc.id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(INSIDE_THE_HOLD);
            let asked_at = Instant::now();
            let answer = save_buffer_content_inner(&state, &id, "written during the hold");
            (asked_at.elapsed(), Instant::now(), answer)
        })
    };

    let announced_at = {
        let deadline = Instant::now() + SETTLE;
        let mut at = None;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(event @ WritEvent::BufferExternal { .. }) if named_by(&event).0 == doc.id => {
                    at = Some(Instant::now());
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        at.expect("the removal was announced")
    };
    let (waited, returned_at, answer) = saver.join().expect("the saving thread");
    let refused = answer.expect_err("the save must be refused");

    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the save returned on the answer, not on a timeout that let it through: {refused}"
    );
    assert!(
        returned_at.duration_since(deleted_at) >= hold_window(NOTES_DEBOUNCE_WINDOW),
        "the save came back before the hold it was waiting on could end"
    );
    assert!(
        waited <= the_longest_a_held_save_may_wait(),
        "a save inside a hold waited {waited:?}, longer than the hold and one window"
    );
    let behind = returned_at.saturating_duration_since(announced_at);
    assert!(
        behind <= released_together(),
        "the save returned {behind:?} after the removal was announced, so it waited out its own \
         deadline instead of being released by the answer"
    );
}

/// Sets a file's modification time to the start of 2020, the way a metadata
/// pass over a restored folder does.
///
/// On APFS that pulls the birth time back with it, on the same inode, which is
/// the whole point of the tests below.
#[cfg(target_os = "macos")]
fn backdate(path: &std::path::Path) {
    let status = std::process::Command::new("touch")
        .arg("-t")
        .arg("202001010000")
        .arg(path)
        .status()
        .expect("run touch");
    assert!(status.success(), "touch -t failed on {}", path.display());
}

#[test]
#[cfg(target_os = "macos")]
fn a_note_whose_birth_time_was_pulled_back_still_follows_its_rename() {
    // A metadata pass that backdates a note in place — `touch -t`, `SetFile
    // -d`, a restore tool — leaves the inode alone and, on APFS, drags the
    // birth time down to the modification time it set. It writes no bytes, so
    // the ignore stamp Writ's own save left takes the event for Writ's and
    // nothing refreshes the id. The record then holds a birth time later than
    // the file's, and reading that as a different file leaves the tab refusing
    // every save over a file sitting at its new name (ADR-033 §12).
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    backdate(&before);
    std::thread::sleep(APART);

    let after = state.notes_root().join("renamed-after-the-backdate.md");
    std::fs::rename(&before, &after).expect("rename the way Finder does");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(
        told.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Moved],
        "a rename after a backdating touch is still a move, saw {seen:?}"
    );
    assert_eq!(
        change_of(told[0]).1.map(std::path::Path::new),
        Some(after.as_path())
    );
    assert!(
        !state.is_removed_on_disk(&doc.id),
        "the tab stopped writing to a file that is there"
    );
    assert_eq!(note_file(&state, &doc.id), after);

    save_buffer_content_inner(&state, &doc.id, "edited after the backdate").expect("save");
    assert_eq!(
        std::fs::read_to_string(&after).expect("read the moved file"),
        "edited after the backdate"
    );
    assert!(
        !before.exists(),
        "the save recreated the file at the old path"
    );
}

/// Writes a file's creation date forward, the way an unarchiver or a restore
/// tool stamping an archive's own date does.
///
/// `SetFile -d` reaches `setattrlist` with `ATTR_CMN_CRTIME`, which moves a
/// live inode's birth time in either direction on APFS. It is the mirror of
/// [`backdate`], and the pair is why macOS reads no birth time: a value that
/// moves both ways cannot be compared against a record to tell a reused inode
/// number from the file that still holds it.
#[cfg(target_os = "macos")]
fn forward_date(path: &std::path::Path) {
    let status = std::process::Command::new("/usr/bin/SetFile")
        .arg("-d")
        .arg("01/01/2090 00:00:00")
        .arg(path)
        .status()
        .expect("run SetFile");
    assert!(status.success(), "SetFile -d failed on {}", path.display());
}

#[test]
#[cfg(target_os = "macos")]
fn a_note_whose_birth_time_was_pushed_forward_still_follows_its_rename() {
    // The mirror of the test above. A restore tool stamping the creation date
    // it read out of an archive writes the birth time forward on the inode the
    // tab is editing, and writes no bytes, so the ignore stamp Writ's own save
    // left takes the event for Writ's and nothing refreshes the id. A record
    // that compared birth times would read the file as a different one and
    // leave the tab refusing every save over a file at its new name.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let before = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    forward_date(&before);
    std::thread::sleep(APART);

    let after = state.notes_root().join("renamed-after-the-forward-date.md");
    std::fs::rename(&before, &after).expect("rename the way Finder does");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(
        told.iter().map(|e| change_of(e).0).collect::<Vec<_>>(),
        vec![&ExternalChange::Moved],
        "a rename after a forward-dating SetFile is still a move, saw {seen:?}"
    );
    assert_eq!(
        change_of(told[0]).1.map(std::path::Path::new),
        Some(after.as_path())
    );
    assert!(
        !state.is_removed_on_disk(&doc.id),
        "the tab stopped writing to a file that is there"
    );
    assert_eq!(note_file(&state, &doc.id), after);

    save_buffer_content_inner(&state, &doc.id, "edited after the forward date").expect("save");
    assert_eq!(
        std::fs::read_to_string(&after).expect("read the moved file"),
        "edited after the forward date"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn a_backdated_note_that_is_deleted_is_still_gone() {
    // The control for the test above. Reading an earlier birth time as the
    // same file must not soften the deletion: the note goes, another file is
    // created in the same window, and that file is younger than the record
    // whatever the record's birth time was pulled back to. Anything else puts
    // the tab on a file nobody opened.
    let dir = TempDir::new().expect("temp dir");
    let (state, rx) = watching_state(&dir);

    let doc = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &doc.id, "text worth keeping").expect("save");
    let path = note_file(&state, &doc.id);
    std::thread::sleep(APART);

    backdate(&path);
    std::thread::sleep(APART);

    let newcomer = state.notes_root().join("aaa-created-in-the-same-window.md");
    std::fs::write(&newcomer, "text worth keeping").expect("the newcomer");
    std::fs::remove_file(&path).expect("delete the note the way Finder does");

    let seen = external_events(&rx);
    let told: Vec<_> = seen.iter().filter(|e| named_by(e).0 == doc.id).collect();
    assert_eq!(told.len(), 1, "the tab must be told once, saw {seen:?}");
    assert_eq!(change_of(told[0]), (&ExternalChange::Removed, None));

    let refused = save_buffer_content_inner(&state, &doc.id, "text worth keeping, edited")
        .expect_err("the save must be refused");
    assert!(
        refused.starts_with(ERR_FILE_REMOVED_ON_DISK),
        "the refusal has to carry its own code, got {refused}"
    );
    assert_eq!(
        std::fs::read_to_string(&newcomer).expect("read the newcomer"),
        "text worth keeping",
        "the save wrote over a file the note was never in"
    );
}
