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
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::{save_buffer_content_inner, ERR_FILE_CHANGED_ON_DISK};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::notes::{
    delete_note_inner, move_notes_folder_to, new_note_inner, note_path_for_id, notes_root_text,
    path_is_inside_notes, rename_note_inner, rename_note_recording, save_note_copy_inner,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::{create_ignore_set, start_notes_watcher};
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
        elsewhere.canonicalize().expect("canonical"),
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
fn watching_state(dir: &TempDir) -> (AppState, mpsc::Receiver<WritEvent>) {
    let state = make_state(dir);
    let (tx, rx) = mpsc::channel();
    state.event_bus.subscribe(move |event| {
        let _ = tx.send(event.clone());
    });

    let open_files = start_open_file_watcher(
        state.event_bus.clone(),
        state.watcher_ignore.clone(),
        &state.notes_root(),
    )
    .expect("start the open file watcher");
    *state.open_file_watcher.lock().expect("watcher slot") = Some(open_files);

    let notes = start_notes_watcher(
        state.event_bus.clone(),
        state.notes_root(),
        state.watcher_ignore.clone(),
        state.open_notes(),
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
