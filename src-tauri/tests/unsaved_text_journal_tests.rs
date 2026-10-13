//! The save command and the unsaved-text journal (ADR-044).
//!
//! A save that does not land leaves the text it carried in the journal before
//! it returns, and the next save of that note that does land clears it. These
//! drive the command layer the frontend calls, for each way a save can fail
//! that can be built on a test machine; the rest are proven on the
//! classification they share (`recovery_tests` in writ-storage).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::file_ops::{FileOpenMode, ERR_OPEN_TOO_LARGE, THRESHOLD_MAX_BYTES};
use writ_core::notes::reload::ChangeChoice;
use writ_core::preview::ContentRendererRegistry;
use writ_core::recovery::UnmadeFile;
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
    close_buffer_inner, delete_buffer_inner, discard_unsaved_text_inner, list_active_buffers_inner,
    read_buffer_content_inner, record_unsaved_notes_inner, resolve_external_change_inner,
    restore_note_file_inner, save_buffer_content_inner, save_note_to_inner, UnsavedNote,
    ERR_FILE_CHANGED_ON_DISK, ERR_FILE_REMOVED_ON_DISK, ERR_NOTE_READ_ONLY,
    ERR_READ_ONLY_DESTINATION, ERR_WRITE_FAILED,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::history::restore_buffer_inner;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{
    canonicalize_for_authorization, resolve_for_containment, AuthorizedPaths,
};
use writ_tauri_lib::state::{restore_unsaved_text, AppState};
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

    let note_history = Arc::new(
        writ_storage::note_history::NoteHistoryStore::open(&writ_dir).expect("version store"),
    );

    AppState {
        store: Mutex::new(BufferStore::new(conn, buffers_dir.clone())),
        config_store: ConfigStore::new(writ_dir.join("config.toml")),
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        notes_root: RwLock::new(notes_root),
        first_run: false,
        first_run_finished: std::sync::atomic::AtomicBool::new(false),
        menu_apps: std::sync::Mutex::new(None),
        retitle_watch: std::sync::Arc::new(writ_tauri_lib::first_run::RetitleWatch::new()),
        notes_root_fallback: RwLock::new(None),
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        notes_watcher: Mutex::new(None),
        open_file_watcher: Mutex::new(None),
        file_tracking: Mutex::new(None),
        notes_index: Arc::new(NotesIndexStore::open(&db_path).expect("notes index db")),
        note_history,
        notes_index_cancel: Arc::new(AtomicBool::new(false)),
        notes_reconcile: Arc::new(ReconcileGate::new()),
        quit: Arc::new(QuitState::new()),
        removal_holds: Default::default(),
        pending_opens: Mutex::new(Vec::new()),
        frontend_ready: AtomicBool::new(false),
        window_revealed: AtomicBool::new(false),
        window_dismissed: AtomicBool::new(false),
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
    }
}

/// Opens `path` holding `content` through the command layer, the way the
/// frontend reaches a file, and returns its note id and canonical path.
fn open_note_at(state: &AppState, path: &Path, content: &str) -> (String, PathBuf) {
    std::fs::write(path, content).expect("write");
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(state, &canonical).expect("open");
    (
        opened.doc.expect("the file opened").id,
        PathBuf::from(canonical),
    )
}

/// What the journal holds for `id`, if anything.
fn journaled(state: &AppState, id: &str) -> Option<String> {
    state
        .store
        .lock()
        .expect("store")
        .unsaved_text()
        .expect("read the journal")
        .entries
        .into_iter()
        .find(|entry| entry.buffer_id == id)
        .map(|entry| entry.text)
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).expect("set permissions");
}

fn dated_copies(dir: &Path, label: &str) -> Vec<PathBuf> {
    let marker = format!("({label} ");
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains(&marker))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn a_save_a_read_only_file_refuses_is_journaled_until_a_save_lands() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (id, path) = open_note_at(&state, &state.notes_root().join("Locked.md"), "on disk\n");

    set_read_only(&path, true);
    let refused = save_buffer_content_inner(&state, &id, "typed once\n").expect_err("read-only");
    assert!(refused.starts_with(ERR_READ_ONLY_DESTINATION), "{refused}");
    assert_eq!(journaled(&state, &id).as_deref(), Some("typed once\n"));

    // Autosave carries on and keeps failing: the entry follows the newest text.
    save_buffer_content_inner(&state, &id, "typed twice\n").expect_err("still read-only");
    assert_eq!(journaled(&state, &id).as_deref(), Some("typed twice\n"));

    set_read_only(&path, false);
    save_buffer_content_inner(&state, &id, "typed twice\n").expect("the save lands");
    assert_eq!(journaled(&state, &id), None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "typed twice\n");
}

#[test]
fn a_save_into_a_folder_that_is_gone_is_journaled() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let volume = state.notes_root().join("Volume");
    std::fs::create_dir_all(&volume).expect("the mounted folder");
    let (id, _) = open_note_at(&state, &volume.join("Remote.md"), "on disk\n");

    // What an unmounted volume looks like from here: the folder is not there.
    std::fs::remove_dir_all(&volume).expect("unmount");
    save_buffer_content_inner(&state, &id, "typed while it was gone\n")
        .expect_err("there is nowhere to write");
    assert_eq!(
        journaled(&state, &id).as_deref(),
        Some("typed while it was gone\n")
    );
}

#[test]
fn a_save_to_a_file_deleted_outside_writ_is_journaled_until_it_is_put_back() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (id, path) = open_note_at(&state, &state.notes_root().join("Gone.md"), "on disk\n");

    std::fs::remove_file(&path).expect("deleted outside Writ");
    state.mark_removed_on_disk(&id);
    let refused = save_buffer_content_inner(&state, &id, "typed after it went\n")
        .expect_err("a deleted file is not recreated by a keystroke");
    assert!(refused.starts_with(ERR_FILE_REMOVED_ON_DISK), "{refused}");
    assert_eq!(
        journaled(&state, &id).as_deref(),
        Some("typed after it went\n")
    );

    restore_note_file_inner(&state, &id, "typed after it went\n").expect("put the file back");
    assert_eq!(journaled(&state, &id), None);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "typed after it went\n"
    );
}

#[test]
fn a_read_only_note_journals_nothing() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("Generated.md");
    std::fs::write(&file, "not anybody's typing").expect("write");
    let now = chrono::Utc::now();
    let doc = BufferDocument {
        id: "generated".to_string(),
        title: "Generated".to_string(),
        filename: "generated.md".to_string(),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(file.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: true,
        size_bytes: 20,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    };
    state.store.lock().unwrap().insert(&doc).expect("insert");

    let refused =
        save_buffer_content_inner(&state, "generated", "edited").expect_err("read-only note");
    assert!(refused.starts_with(ERR_NOTE_READ_ONLY), "{refused}");
    assert_eq!(journaled(&state, "generated"), None);
}

#[test]
fn a_generated_document_the_last_0_5_snapshot_held_leaves_nothing_in_the_journal() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let file = notes.join("Third-party licences.md");
    std::fs::write(&file, "generated\n").expect("write");
    let opened_at = chrono::Utc::now() - chrono::Duration::seconds(30);
    let doc = BufferDocument {
        id: "licences".to_string(),
        title: "Third-party licences".to_string(),
        filename: "licences.md".to_string(),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(file.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: opened_at,
        updated_at: opened_at,
        closed_at: None,
        read_only: true,
        size_bytes: 10,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    };
    state.store.lock().unwrap().insert(&doc).expect("insert");
    let conn = open_database(&dir.path().join("writ.db")).expect("open db");
    conn.execute(
        "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
         VALUES ('heartbeat', 1, ?1, datetime('now'), 0)",
        [r#"{"buffers":{"licences":"generated\n"}}"#],
    )
    .expect("the snapshot 0.5 left");

    for launch in ["first", "second"] {
        let restored = {
            let store = state.store.lock().expect("store");
            restore_unsaved_text(&store, &notes, state.default_extension())
        };
        assert!(restored.recovered.is_empty(), "{launch} launch");
        assert_eq!(
            state.store.lock().unwrap().unsaved_text_count().unwrap(),
            0,
            "{launch} launch: an entry the launch cannot write would never clear"
        );
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "generated\n");
    assert!(dated_copies(&notes, "recovered").is_empty());
}

#[test]
fn a_refused_save_whose_text_went_beside_the_note_clears_the_entry() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Shared.md"), "on disk\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed\n").expect_err("read-only");
    assert!(journaled(&state, &id).is_some());

    // The file comes back writable holding what another device wrote.
    set_read_only(&path, false);
    std::fs::write(&path, "what a sync client delivered\n").expect("external write");
    let refused = save_buffer_content_inner(&state, &id, "typed more\n")
        .expect_err("the guard stops a write over a change Writ never read");
    assert!(refused.starts_with(ERR_FILE_CHANGED_ON_DISK), "{refused}");

    let copies = dated_copies(&notes, "conflict");
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), "typed more\n");
    assert_eq!(
        journaled(&state, &id),
        None,
        "the copy is a file holding the text, so the journal keeps no second one"
    );
}

#[test]
fn an_answer_that_keeps_the_files_version_clears_the_entry() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Answered.md"), "on disk\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "mine\n").expect_err("read-only");
    set_read_only(&path, false);
    std::fs::write(&path, "theirs\n").expect("external write");

    let outcome = resolve_external_change_inner(&state, &id, ChangeChoice::UseDisk, "mine\n")
        .expect("the answer lands");
    assert_eq!(outcome.content.as_deref(), Some("theirs\n"));
    let copy = outcome
        .conflict_copy_path
        .expect("mine went beside the note");
    assert_eq!(std::fs::read_to_string(copy).unwrap(), "mine\n");
    assert_eq!(journaled(&state, &id), None);
}

#[test]
fn text_the_person_chose_to_lose_does_not_come_back_at_the_next_launch() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Lost.md"), "on disk\n");
    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed\n").expect_err("read-only");
    assert!(journaled(&state, &id).is_some());

    // "Close and lose changes".
    discard_unsaved_text_inner(&state, std::slice::from_ref(&id)).expect("discard");
    close_buffer_inner(&state, &id).expect("close");
    set_read_only(&path, false);

    assert_eq!(journaled(&state, &id), None);
    let restored = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    assert!(restored.recovered.is_empty());
    assert!(dated_copies(&notes, "recovered").is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
}

/// The frontend reaches these by name through `invoke`, so a command dropped
/// from the handler compiles and every test here still passes, while text the
/// person chose to lose comes back at the next launch.
#[test]
fn the_journal_commands_are_in_the_invoke_handler() {
    const LIB_RS: &str = include_str!("../src/lib.rs");

    for command in [
        "commands::buffer::record_unsaved_notes,",
        "commands::buffer::discard_unsaved_text,",
        "commands::buffer::save_note_to,",
        "commands::file::pick_note_save_path,",
    ] {
        assert!(
            LIB_RS.contains(command),
            "{} is not in the invoke handler, so the frontend cannot call it",
            command.trim_end_matches(',')
        );
    }
}

/// A note with no file yet, the way a new tab starts (`create_buffer`).
fn insert_new_note(state: &AppState, id: &str, title: &str) {
    let now = chrono::Utc::now();
    let doc = BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
        filename: format!("{id}.txt"),
        status: BufferStatus::Active,
        language: None,
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    };
    state.store.lock().unwrap().insert(&doc).expect("insert");
}

/// Locks a folder against new files (`0o555`) or unlocks it (`0o755`).
#[cfg(unix)]
fn set_folder_mode(folder: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode))
        .expect("set the folder's mode");
}

/// The file a note titled `title` gets first in this state's notes folder,
/// as the bar names it when the launch could not make it.
fn unmade(state: &AppState, title: &str) -> UnmadeFile {
    UnmadeFile {
        name: format!("{title}.{}", state.default_extension().as_str()),
        folder: state
            .notes_root()
            .file_name()
            .expect("the notes folder has a name")
            .to_string_lossy()
            .into_owned(),
    }
}

/// The file note `id`'s row names, if it names one.
fn row_path(state: &AppState, id: &str) -> Option<String> {
    state
        .store
        .lock()
        .unwrap()
        .get(id)
        .expect("the note's row")
        .source_path
}

#[cfg(unix)]
#[test]
fn a_new_note_whose_first_file_cannot_be_made_gets_one_at_the_next_launch() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    insert_new_note(&state, "fresh", "Fresh");

    set_folder_mode(&notes, 0o555);
    let refused = save_buffer_content_inner(&state, "fresh", "the first words");
    set_folder_mode(&notes, 0o755);

    refused.expect_err("the notes folder took no new file");
    assert_eq!(
        journaled(&state, "fresh").as_deref(),
        Some("the first words")
    );
    assert_eq!(
        std::fs::read_dir(&notes).expect("read_dir").count(),
        0,
        "the refused save made no file"
    );

    let restored = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    let offered = restored
        .recovered
        .iter()
        .find(|buf| buf.id == "fresh")
        .expect("the launch restores the text");
    assert!(
        !offered.removed_on_disk,
        "a note whose first file was never made came back as deleted"
    );
    let path = row_path(&state, "fresh").expect("the launch gave the note its file");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the first words");
    assert_eq!(Path::new(&path).parent(), Some(notes.as_path()));
    assert_eq!(journaled(&state, "fresh"), None);
}

#[cfg(unix)]
#[test]
fn a_new_note_whose_first_file_cannot_be_made_gets_one_from_the_next_save() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    insert_new_note(&state, "fresh", "Fresh");

    set_folder_mode(&notes, 0o555);
    let refused = save_buffer_content_inner(&state, "fresh", "the first words");
    set_folder_mode(&notes, 0o755);

    refused.expect_err("the notes folder took no new file");
    assert_eq!(
        row_path(&state, "fresh"),
        None,
        "the note names a file that was never made"
    );

    save_buffer_content_inner(&state, "fresh", "the first words")
        .expect("the folder takes the file now");
    let path = row_path(&state, "fresh").expect("the save gave the note its file");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the first words");
    assert_eq!(journaled(&state, "fresh"), None);
}

#[cfg(unix)]
#[test]
fn a_new_note_whose_text_cannot_be_journaled_keeps_the_path_its_save_minted() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    insert_new_note(&state, "fresh", "Fresh");
    open_database(&dir.path().join("writ.db"))
        .expect("open db")
        .execute("DROP TABLE unsaved_text", [])
        .expect("the journal can take nothing");

    set_folder_mode(&notes, 0o555);
    let refused = save_buffer_content_inner(&state, "fresh", "the first words");
    set_folder_mode(&notes, 0o755);

    refused.expect_err("the notes folder took no new file");
    assert!(
        row_path(&state, "fresh").is_some(),
        "a note whose text is nowhere was left looking like an empty one"
    );
}

#[cfg(unix)]
#[test]
fn a_launch_that_cannot_make_a_new_notes_first_file_hands_the_text_to_its_tab() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    insert_new_note(&state, "fresh", "Fresh");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text("fresh", "the first words", chrono::Utc::now())
        .expect("journal");

    set_folder_mode(&notes, 0o555);
    let restored = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    set_folder_mode(&notes, 0o755);

    let handed = restored
        .recovered
        .iter()
        .find(|buf| buf.id == "fresh")
        .expect("the tab is handed the text");
    assert!(
        handed.removed_on_disk,
        "the tab came up without the bar that holds its saves"
    );
    assert_eq!(handed.content, "the first words");
    assert_eq!(
        handed.unmade_file,
        Some(unmade(&state, "Fresh")),
        "the tab names the file that could not be made, not a deleted one"
    );
    assert_eq!(
        journaled(&state, "fresh").as_deref(),
        Some("the first words")
    );
    assert_eq!(row_path(&state, "fresh"), None);

    restore_note_file_inner(&state, "fresh", "the first words")
        .expect("the folder takes the file now");
    let path = row_path(&state, "fresh").expect("putting the file back gave the note its file");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the first words");
    assert_eq!(journaled(&state, "fresh"), None);
}

#[cfg(unix)]
#[test]
fn a_new_note_a_launch_could_not_give_a_file_gets_one_at_the_next() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    insert_new_note(&state, "fresh", "Fresh");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text("fresh", "the first words", chrono::Utc::now())
        .expect("journal");

    set_folder_mode(&notes, 0o555);
    let first = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    set_folder_mode(&notes, 0o755);

    assert!(
        first
            .recovered
            .iter()
            .any(|buf| buf.id == "fresh" && buf.removed_on_disk),
        "the tab is handed the text"
    );
    assert_eq!(
        journaled(&state, "fresh").as_deref(),
        Some("the first words"),
        "the next launch tries again"
    );

    let second = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    let offered = second
        .recovered
        .iter()
        .find(|buf| buf.id == "fresh")
        .expect("the next launch restores the text");
    assert!(
        !offered.removed_on_disk,
        "a note whose first file was never made came back as deleted"
    );
    let path = row_path(&state, "fresh").expect("the launch gave the note its file");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the first words");
    assert_eq!(journaled(&state, "fresh"), None);
}

#[test]
fn a_launch_that_cannot_name_a_new_notes_first_file_hands_the_text_to_its_tab() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    insert_new_note(&state, "fresh", "Fresh");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text("fresh", "the first words", chrono::Utc::now())
        .expect("journal");
    let conn = open_database(&dir.path().join("writ.db")).expect("open db");
    conn.execute_batch(
        "CREATE TRIGGER no_file_for_a_row BEFORE UPDATE OF source_path ON buffers
         BEGIN SELECT RAISE(ABORT, 'no file for a row'); END;",
    )
    .expect("a row can be given no file");

    let restored = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    let handed = restored
        .recovered
        .iter()
        .find(|buf| buf.id == "fresh")
        .expect("the tab is handed the text");
    assert!(
        handed.removed_on_disk,
        "the tab came up without the bar that holds its saves"
    );
    assert_eq!(handed.content, "the first words");
    assert_eq!(handed.unmade_file, Some(unmade(&state, "Fresh")));
    assert_eq!(
        journaled(&state, "fresh").as_deref(),
        Some("the first words")
    );

    conn.execute_batch("DROP TRIGGER no_file_for_a_row;")
        .expect("rows take files again");
    let next = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    let offered = next
        .recovered
        .iter()
        .find(|buf| buf.id == "fresh")
        .expect("the next launch restores the text");
    assert!(!offered.removed_on_disk);
    let path = row_path(&state, "fresh").expect("the launch gave the note its file");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the first words");
    assert_eq!(journaled(&state, "fresh"), None);
}

/// Collects what the restore logs.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_restore_logs_and_keeps_an_entry_it_cannot_decode() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (good, good_path) = open_note_at(&state, &notes.join("Good.md"), "on disk\n");
    let (bad, _) = open_note_at(&state, &notes.join("Bad.md"), "on disk\n");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text(&good, "typed\n", chrono::Utc::now())
        .expect("journal");
    let conn = open_database(&dir.path().join("writ.db")).expect("open db");
    conn.execute(
        "INSERT INTO unsaved_text (buffer_id, path, text, text_hash, recorded_at)
         VALUES (?1, NULL, CAST(X'FFFE' AS TEXT), 'hash', '2026-10-13T00:00:00+00:00')",
        [&bad],
    )
    .expect("insert text that is not UTF-8");

    let logs = Captured::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let restored = tracing::subscriber::with_default(subscriber, || {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    });

    let logged = String::from_utf8_lossy(&logs.0.lock().unwrap()).into_owned();
    assert!(
        logged.contains("could not be decoded"),
        "nothing in the log names the entry that could not be read:\n{logged}"
    );
    let ids: Vec<&str> = restored
        .recovered
        .iter()
        .map(|buf| buf.id.as_str())
        .collect();
    assert_eq!(ids, vec![good.as_str()]);
    assert_eq!(
        std::fs::read_to_string(&dated_copies(&notes, "recovered")[0]).unwrap(),
        "typed\n"
    );
    assert_eq!(std::fs::read_to_string(&good_path).unwrap(), "on disk\n");

    let kept: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM unsaved_text WHERE buffer_id = ?1",
            [&bad],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(kept, 1, "the entry that could not be read is still there");
    assert_eq!(
        journaled(&state, &good),
        None,
        "the restored entry was cleared"
    );
}

#[cfg(unix)]
#[test]
fn a_restore_that_cannot_write_keeps_the_entry_and_is_not_counted() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let folder = notes.join("Locked");
    std::fs::create_dir_all(&folder).expect("folder");
    let (id, _) = open_note_at(&state, &folder.join("Note.md"), "on disk\n");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text(&id, "typed\n", chrono::Utc::now())
        .expect("journal");

    // The file holds other text, so the restore writes beside it, and the
    // folder takes no new file.
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555))
        .expect("lock the folder");
    let restored = {
        let store = state.store.lock().expect("store");
        restore_unsaved_text(&store, &notes, state.default_extension())
    };
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755))
        .expect("unlock the folder");

    assert!(
        restored.recovered.is_empty(),
        "the toast would claim a restore that did not happen"
    );
    assert_eq!(
        journaled(&state, &id).as_deref(),
        Some("typed\n"),
        "the next launch tries again"
    );
}

/// Closes note `id`'s tab the way Cmd+W does after a save the guard refused:
/// the row is closed first, then the frontend hands over the text only it
/// still holds (`forgetClosed`, `keepUnsavedForRecovery`).
fn close_tab_handing_over(state: &AppState, id: &str, text: &str) {
    close_buffer_inner(state, id).expect("close the tab");
    record_unsaved_notes_inner(
        state,
        vec![UnsavedNote {
            id: id.to_string(),
            content: text.to_string(),
        }],
    )
    .expect("hand the text over");
}

/// Opens the file at `path` again through the command layer, which brings
/// back the closed note's row, and returns the note's id.
fn reopen_at(state: &AppState, path: &Path) -> String {
    let canonical = path.to_string_lossy().into_owned();
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id
}

/// What the next launch does with the journal, run against this state's store.
fn relaunch(state: &AppState) -> writ_tauri_lib::state::RestoredText {
    let store = state.store.lock().expect("store");
    restore_unsaved_text(&store, &state.notes_root(), state.default_extension())
}

/// The text of every dated copy labelled `label` in `dir`, sorted.
fn copy_texts(dir: &Path, label: &str) -> Vec<String> {
    let mut texts: Vec<String> = dated_copies(dir, label)
        .iter()
        .map(|path| std::fs::read_to_string(path).expect("read the copy"))
        .collect();
    texts.sort();
    texts
}

/// Every text the journal holds for `id`, oldest first.
fn journal_texts(state: &AppState, id: &str) -> Vec<String> {
    state
        .store
        .lock()
        .expect("store")
        .unsaved_text()
        .expect("read the journal")
        .entries
        .into_iter()
        .filter(|entry| entry.buffer_id == id)
        .map(|entry| entry.text)
        .collect()
}

fn journal_count(state: &AppState) -> usize {
    state
        .store
        .lock()
        .expect("store")
        .unsaved_text_count()
        .expect("count the journal")
}

/// Cmd+W after a read-only refusal closes the tab without asking, because
/// that refusal is not retried, and hands its text over. The note reopened
/// later in the session shows its file, so none of the reopened tab's saves
/// carries that text.
#[test]
fn text_a_closed_tab_handed_over_survives_a_reopen_and_a_save_that_lands() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Shared.md"), "on disk\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed before the close\n").expect_err("read-only");
    close_tab_handing_over(&state, &id, "typed before the close\n");

    set_read_only(&path, false);
    assert_eq!(
        reopen_at(&state, &path),
        id,
        "the reopen brings the row back"
    );
    save_buffer_content_inner(&state, &id, "on disk\ntyped after the reopen\n")
        .expect("the save lands");

    let restored = relaunch(&state);
    assert_eq!(
        copy_texts(&notes, "recovered"),
        vec!["typed before the close\n"],
        "the text typed before the close is on disk after the launch"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "on disk\ntyped after the reopen\n"
    );
    assert_eq!(restored.recovered.len(), 1);
    assert_eq!(journal_count(&state), 0);
}

/// The same close and reopen with the file still read-only. The reopened
/// tab's refused save is text of its own, kept beside what the closed tab
/// handed over rather than in place of it.
#[test]
fn a_refused_save_after_a_reopen_keeps_the_text_an_earlier_tab_handed_over() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Shared.md"), "on disk\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed before the close\n").expect_err("read-only");
    close_tab_handing_over(&state, &id, "typed before the close\n");

    assert_eq!(
        reopen_at(&state, &path),
        id,
        "the reopen brings the row back"
    );
    save_buffer_content_inner(&state, &id, "on disk\ntyped after the reopen\n")
        .expect_err("still read-only");
    set_read_only(&path, false);

    relaunch(&state);
    assert_eq!(
        copy_texts(&notes, "recovered"),
        vec![
            "on disk\ntyped after the reopen\n",
            "typed before the close\n"
        ],
        "both texts are on disk after the launch"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
    assert_eq!(journal_count(&state), 0);
}

/// A launch that could not write a note's text leaves the note's tab showing
/// its file, so none of that tab's saves carries the text either.
#[cfg(unix)]
#[test]
fn text_a_launch_could_not_write_survives_a_save_of_its_note() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let folder = state.notes_root().join("Locked");
    std::fs::create_dir_all(&folder).expect("folder");
    let (id, path) = open_note_at(&state, &folder.join("Note.md"), "on disk\n");
    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed\n").expect_err("read-only");
    set_read_only(&path, false);

    // The file holds other text, so the launch writes beside it, and the
    // folder takes no new file.
    set_folder_mode(&folder, 0o555);
    let first = relaunch(&state);
    set_folder_mode(&folder, 0o755);
    assert!(first.recovered.is_empty(), "nothing was restored");

    save_buffer_content_inner(&state, &id, "on disk\ntyped after the launch\n")
        .expect("the save lands");

    relaunch(&state);
    assert_eq!(
        copy_texts(&folder, "recovered"),
        vec!["typed\n"],
        "the text the first launch could not write is on disk after the next"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "on disk\ntyped after the launch\n"
    );
    assert_eq!(journal_count(&state), 0);
}

/// Text kept for a note whose file was deleted outside Writ, with the note's
/// tab closed. The file comes back, the person opens it, and the tab shows
/// what came back.
#[test]
fn text_kept_for_a_deleted_file_survives_its_return_a_reopen_and_a_save() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Gone.md"), "on disk\n");

    std::fs::remove_file(&path).expect("deleted outside Writ");
    state.mark_removed_on_disk(&id);
    save_buffer_content_inner(&state, &id, "typed after it went\n")
        .expect_err("a deleted file is not recreated by a keystroke");
    close_tab_handing_over(&state, &id, "typed after it went\n");

    let first = relaunch(&state);
    let offered: Vec<_> = first.recovered.iter().filter(|buf| buf.id == id).collect();
    assert_eq!(offered.len(), 1);
    assert!(offered[0].removed_on_disk);
    assert_eq!(offered[0].content, "typed after it went\n");

    std::fs::write(&path, "back from the trash\n").expect("the file comes back");
    assert_eq!(
        reopen_at(&state, &path),
        id,
        "the reopen brings the row back"
    );
    save_buffer_content_inner(&state, &id, "back from the trash\nedited\n")
        .expect("the save lands");

    relaunch(&state);
    assert_eq!(
        copy_texts(&notes, "recovered"),
        vec!["typed after it went\n"],
        "the text kept while the file was gone is on disk after the launch"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "back from the trash\nedited\n"
    );
    assert_eq!(journal_count(&state), 0);
}

/// "Close and lose changes" on the reopened tab loses the text that tab held.
/// What the earlier tab handed over was never in it.
#[test]
fn losing_a_reopened_tabs_changes_keeps_what_an_earlier_tab_handed_over() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Shared.md"), "on disk\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed before the close\n").expect_err("read-only");
    close_tab_handing_over(&state, &id, "typed before the close\n");

    assert_eq!(
        reopen_at(&state, &path),
        id,
        "the reopen brings the row back"
    );
    save_buffer_content_inner(&state, &id, "on disk\ntyped after the reopen\n")
        .expect_err("still read-only");
    discard_unsaved_text_inner(&state, std::slice::from_ref(&id)).expect("discard");
    close_buffer_inner(&state, &id).expect("close");
    set_read_only(&path, false);

    relaunch(&state);
    assert_eq!(
        copy_texts(&notes, "recovered"),
        vec!["typed before the close\n"]
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
    assert_eq!(journal_count(&state), 0);
}

/// Two texts kept for a note whose file is gone: the one a closed tab handed
/// over, and the one the tab the history list brought back could not save.
/// The tab is handed the newer, which "Put the file back" writes. The older is
/// still kept, and goes beside the file at the next launch.
#[test]
fn a_note_whose_file_is_gone_gets_its_newest_text_and_keeps_the_older() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Gone.md"), "on disk\n");

    std::fs::remove_file(&path).expect("deleted outside Writ");
    state.mark_removed_on_disk(&id);
    save_buffer_content_inner(&state, &id, "typed before the close\n")
        .expect_err("a deleted file is not recreated by a keystroke");
    close_tab_handing_over(&state, &id, "typed before the close\n");

    // The history list brings the row back (`restore_buffer`), and the file
    // is still gone.
    state.store.lock().unwrap().restore(&id).expect("reopen");
    state.mark_removed_on_disk(&id);
    save_buffer_content_inner(&state, &id, "typed after the reopen\n")
        .expect_err("a deleted file is not recreated by a keystroke");

    let first = relaunch(&state);
    let offered: Vec<_> = first.recovered.iter().filter(|buf| buf.id == id).collect();
    assert_eq!(offered.len(), 1, "the tab is handed one text");
    assert!(offered[0].removed_on_disk);
    assert_eq!(
        offered[0].unmade_file, None,
        "a deleted file is named as deleted"
    );
    assert_eq!(offered[0].content, "typed after the reopen\n");
    assert_eq!(
        journal_texts(&state, &id),
        vec!["typed before the close\n", "typed after the reopen\n"],
        "both texts are kept"
    );

    restore_note_file_inner(&state, &id, "typed after the reopen\n").expect("put the file back");
    assert_eq!(
        journal_texts(&state, &id),
        vec!["typed before the close\n"],
        "putting the file back settles the text the tab was handed"
    );

    relaunch(&state);
    assert_eq!(
        copy_texts(&notes, "recovered"),
        vec!["typed before the close\n"]
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "typed after the reopen\n"
    );
    assert_eq!(journal_count(&state), 0);
}

/// Every note is tried, and the call fails once the rest are kept, so the
/// editor keeps its own copy of the text the journal could not take.
#[test]
fn a_handover_that_cannot_keep_one_notes_text_keeps_the_rest_and_fails() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (kept, _) = open_note_at(&state, &notes.join("Kept.md"), "on disk\n");
    let (refused, _) = open_note_at(&state, &notes.join("Refused.md"), "on disk\n");
    let conn = open_database(&dir.path().join("writ.db")).expect("open db");
    conn.execute_batch(&format!(
        "CREATE TRIGGER refuse_one BEFORE INSERT ON unsaved_text
         WHEN NEW.buffer_id = '{refused}'
         BEGIN SELECT RAISE(ABORT, 'the journal refuses this note'); END;"
    ))
    .expect("make the journal refuse one note");

    let error = record_unsaved_notes_inner(
        &state,
        vec![
            UnsavedNote {
                id: refused.clone(),
                content: "not kept\n".to_string(),
            },
            UnsavedNote {
                id: kept.clone(),
                content: "kept\n".to_string(),
            },
        ],
    )
    .expect_err("one note's text could not be kept");
    assert!(error.starts_with(ERR_WRITE_FAILED), "{error}");
    assert!(error.contains("1 of 2"), "{error}");
    assert_eq!(
        journal_texts(&state, &kept),
        vec!["kept\n"],
        "the note after the one that failed is still tried"
    );
    assert!(journal_texts(&state, &refused).is_empty());
}

/// "Remove from history" deletes the note's row and leaves the text its tab
/// handed over: an entry keeps its own lifetime, and the next launch writes
/// it beside the file it was typed for (ADR-044 §3).
#[test]
fn text_whose_history_row_was_removed_comes_back_at_the_next_launch() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Removed.md"), "on disk\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed before the close\n").expect_err("read-only");
    close_tab_handing_over(&state, &id, "typed before the close\n");
    set_read_only(&path, false);
    delete_buffer_inner(&state, &id).expect("remove the note from history");
    assert_eq!(
        journal_texts(&state, &id),
        vec!["typed before the close\n"],
        "deleting the row leaves the text"
    );

    let restored = relaunch(&state);
    assert_eq!(
        copy_texts(&notes, "recovered"),
        vec!["typed before the close\n"],
        "the text is on disk beside the file after the launch"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
    assert_eq!(restored.recovered.len(), 1, "the toast counts it");
    assert!(!restored.recovered[0].removed_on_disk);
    assert_eq!(journal_count(&state), 0);
}

/// The same for a note that never had a file: there is no file to write
/// beside, so the launch gives the text a file of its own in the notes
/// folder.
#[test]
fn text_of_a_note_with_no_file_whose_row_was_removed_gets_a_file_of_its_own() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    insert_new_note(&state, "fresh", "writ-1760000000000");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text("fresh", "the only copy\n", chrono::Utc::now())
        .expect("journal");
    close_buffer_inner(&state, "fresh").expect("close the tab");
    delete_buffer_inner(&state, "fresh").expect("remove the note from history");

    let restored = relaunch(&state);
    let written: Vec<String> = std::fs::read_dir(&notes)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .map(|path| std::fs::read_to_string(path).expect("read the file"))
        .collect();
    assert_eq!(written, vec!["the only copy\n"]);
    assert_eq!(restored.recovered.len(), 1);
    assert_eq!(journal_count(&state), 0);
}

/// A save the guard stopped wrote its text beside the note, and the tab still
/// holds that text when it closes or Writ quits. The dated copy already holds
/// it, so the handover keeps nothing and the next launch writes no second
/// copy. Text the copy does not hold is kept as before (ADR-033 §15).
#[test]
fn text_a_dated_copy_already_holds_is_not_handed_over_again() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Shared.md"), "on disk\n");
    std::fs::write(&path, "what a sync client delivered\n").expect("external write");
    let refused = save_buffer_content_inner(&state, &id, "typed\n")
        .expect_err("the guard stops a write over a change Writ never read");
    assert!(refused.starts_with(ERR_FILE_CHANGED_ON_DISK), "{refused}");
    assert_eq!(copy_texts(&notes, "conflict"), vec!["typed\n"]);
    let (other, _) = open_note_at(&state, &notes.join("Other.md"), "other\n");

    record_unsaved_notes_inner(
        &state,
        vec![
            UnsavedNote {
                id: id.clone(),
                content: "typed\n".to_string(),
            },
            UnsavedNote {
                id: other.clone(),
                content: "held for the other\n".to_string(),
            },
        ],
    )
    .expect("hand the text over");
    assert_eq!(
        journaled(&state, &id),
        None,
        "the dated copy holds this text"
    );
    assert_eq!(
        journaled(&state, &other).as_deref(),
        Some("held for the other\n")
    );

    relaunch(&state);
    assert_eq!(
        copy_texts(&notes, "recovered"),
        vec!["held for the other\n"],
        "no second copy of the text the guard set aside"
    );
    assert_eq!(copy_texts(&notes, "conflict"), vec!["typed\n"]);

    record_unsaved_notes_inner(
        &state,
        vec![UnsavedNote {
            id: id.clone(),
            content: "typed\nand more\n".to_string(),
        }],
    )
    .expect("hand the text over");
    assert_eq!(
        journaled(&state, &id).as_deref(),
        Some("typed\nand more\n"),
        "text typed after the copy is kept"
    );
}

/// Two texts of one note come back as two copies, and the launch counts the
/// note once: the toast counts notes, not texts (ADR-044 §4).
#[test]
fn a_note_restored_from_two_texts_counts_once() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Twice.md"), "on disk\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed before the close\n").expect_err("read-only");
    close_tab_handing_over(&state, &id, "typed before the close\n");
    assert_eq!(reopen_at(&state, &path), id);
    save_buffer_content_inner(&state, &id, "on disk\ntyped after the reopen\n")
        .expect_err("still read-only");
    set_read_only(&path, false);

    let restored = relaunch(&state);
    assert_eq!(copy_texts(&notes, "recovered").len(), 2);
    let counted: Vec<&str> = restored
        .recovered
        .iter()
        .filter(|note| !note.removed_on_disk)
        .map(|note| note.id.as_str())
        .collect();
    assert_eq!(counted, vec![id.as_str()]);
    assert_eq!(journal_count(&state), 0);
}

/// A path outside the notes folder, resolved the way the save panel's answer
/// is ([`writ_tauri_lib::commands::file::pick_note_save_path`]).
fn chosen_path(dir: &Path) -> String {
    resolve_for_containment(dir).expect("the chosen path resolves")
}

/// "Save As…" on the bar of a note whose first file could not be made: the
/// place the person picked takes the text, the row names it, and the journal
/// entry clears. A place nobody picked is refused and changes nothing.
#[test]
fn save_as_gives_a_note_with_no_file_its_first_file_where_the_person_chose() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    insert_new_note(&state, "fresh", "Fresh");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text("fresh", "the first words", chrono::Utc::now())
        .expect("journal");
    let elsewhere = dir.path().join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("folder");
    let chosen = chosen_path(&elsewhere.join("Chosen.txt"));

    save_note_to_inner(&state, "fresh", "the first words", &chosen)
        .expect_err("a place outside the notes folder nobody picked");
    assert_eq!(row_path(&state, "fresh"), None);
    assert!(!Path::new(&chosen).exists());
    assert_eq!(
        journaled(&state, "fresh").as_deref(),
        Some("the first words")
    );

    state.authorized_paths.record_blessed_source(chosen.clone());
    let disk_hash = save_note_to_inner(&state, "fresh", "the first words", &chosen)
        .expect("the place the person picked takes the text");
    assert!(disk_hash.is_some());
    assert_eq!(std::fs::read_to_string(&chosen).unwrap(), "the first words");
    assert_eq!(row_path(&state, "fresh").as_deref(), Some(chosen.as_str()));
    assert_eq!(journaled(&state, "fresh"), None);

    let second = chosen_path(&elsewhere.join("Second.txt"));
    state.authorized_paths.record_blessed_source(second.clone());
    save_note_to_inner(&state, "fresh", "more words", &second)
        .expect_err("a note that has a file is not given a second one");
    assert_eq!(row_path(&state, "fresh").as_deref(), Some(chosen.as_str()));
    assert!(!Path::new(&second).exists());
}

/// The save panel asks before a file is replaced, so a file at the chosen
/// place is one the person chose to replace.
#[test]
fn save_as_over_a_file_the_person_chose_to_replace_lands() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    insert_new_note(&state, "fresh", "Fresh");
    let elsewhere = dir.path().join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("folder");
    std::fs::write(elsewhere.join("Old.txt"), "what the file held\n").expect("write");
    let chosen = chosen_path(&elsewhere.join("Old.txt"));
    state.authorized_paths.record_blessed_source(chosen.clone());

    save_note_to_inner(&state, "fresh", "the first words", &chosen).expect("replaced");
    assert_eq!(std::fs::read_to_string(&chosen).unwrap(), "the first words");
    assert_eq!(row_path(&state, "fresh").as_deref(), Some(chosen.as_str()));
    assert!(
        dated_copies(&elsewhere, "conflict").is_empty(),
        "Writ held no record of the file, so the guard let the write land"
    );
}

/// A file another tab has open stays that tab's: Save As does not give a
/// second note the same file.
#[test]
fn save_as_does_not_take_a_file_another_tab_has_open() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (_, open_path) = open_note_at(&state, &notes.join("Open.md"), "the open tab's text\n");
    insert_new_note(&state, "fresh", "Fresh");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text("fresh", "the first words", chrono::Utc::now())
        .expect("journal");

    let refused = save_note_to_inner(
        &state,
        "fresh",
        "the first words",
        &open_path.to_string_lossy(),
    )
    .expect_err("a file another tab has open is not replaced");
    assert!(refused.starts_with(ERR_WRITE_FAILED), "{refused}");
    assert_eq!(row_path(&state, "fresh"), None);
    assert_eq!(
        std::fs::read_to_string(&open_path).unwrap(),
        "the open tab's text\n"
    );
    assert_eq!(
        journaled(&state, "fresh").as_deref(),
        Some("the first words")
    );
}

/// A write to the chosen place that does not land leaves the note as it was:
/// no file on its row, its text in the journal, and a file the person meant
/// to replace untouched.
#[test]
fn save_as_that_does_not_land_takes_the_chosen_path_back() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    insert_new_note(&state, "fresh", "Fresh");
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text("fresh", "the first words", chrono::Utc::now())
        .expect("journal");

    let gone = chosen_path(&dir.path().join("Gone").join("Chosen.txt"));
    state.authorized_paths.record_blessed_source(gone.clone());
    save_note_to_inner(&state, "fresh", "the first words", &gone)
        .expect_err("a folder that is not there takes no file");
    assert_eq!(row_path(&state, "fresh"), None);

    let elsewhere = dir.path().join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("folder");
    std::fs::write(elsewhere.join("Locked.txt"), "what the file held\n").expect("write");
    let locked = chosen_path(&elsewhere.join("Locked.txt"));
    set_read_only(Path::new(&locked), true);
    state.authorized_paths.record_blessed_source(locked.clone());
    let refused = save_note_to_inner(&state, "fresh", "the first words", &locked)
        .expect_err("a read-only file is not replaced");
    assert!(refused.starts_with(ERR_READ_ONLY_DESTINATION), "{refused}");
    set_read_only(Path::new(&locked), false);
    assert_eq!(row_path(&state, "fresh"), None);
    assert_eq!(
        std::fs::read_to_string(&locked).unwrap(),
        "what the file held\n"
    );
    assert_eq!(
        journaled(&state, "fresh").as_deref(),
        Some("the first words")
    );
}

/// A closed file that Save As gave to another note is that note's now:
/// reopening it from the history list answers with the tab that has it open,
/// and the closed row stays closed, so the file has one open tab.
#[test]
fn reopening_a_closed_file_another_tab_has_open_answers_with_that_tab() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (closed, _) = open_note_at(&state, &notes.join("Hist.md"), "what the file held\n");
    close_buffer_inner(&state, &closed).expect("close the tab");
    insert_new_note(&state, "fresh", "Fresh");
    let chosen = row_path(&state, &closed).expect("the closed row names its file");
    save_note_to_inner(&state, "fresh", "the first words\n", &chosen).expect("Save As lands");

    let reopened = restore_buffer_inner(&state, &closed).expect("an answer");

    assert_eq!(reopened.doc.expect("a tab").id, "fresh");
    let store = state.store.lock().unwrap();
    assert_eq!(
        store.get(&closed).unwrap().status,
        BufferStatus::History,
        "the closed row stays closed"
    );
    let open: Vec<String> = store
        .list_by_status(BufferStatus::Active)
        .unwrap()
        .into_iter()
        .filter(|row| row.source_path.as_deref() == Some(chosen.as_str()))
        .map(|row| row.id)
        .collect();
    assert_eq!(open, vec!["fresh".to_string()]);
}

/// Text whose history row was removed goes beside its file while that file is
/// there, wherever it is: a file outside the notes folder keeps its copy in
/// its own folder (ADR-044 §4, `plan_orphan_recovery`).
#[test]
fn removed_text_goes_beside_its_file_outside_the_notes_folder() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let away = dir.path().join("Away");
    std::fs::create_dir_all(&away).expect("folder");
    let (id, path) = open_note_at(&state, &away.join("Away.md"), "on disk\n");
    let folder = path.parent().expect("the file's folder").to_path_buf();

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed away\n").expect_err("read-only");
    close_tab_handing_over(&state, &id, "typed away\n");
    set_read_only(&path, false);
    delete_buffer_inner(&state, &id).expect("remove the note from history");

    let restored = relaunch(&state);
    assert_eq!(copy_texts(&folder, "recovered"), vec!["typed away\n"]);
    assert!(
        dated_copies(&notes, "recovered").is_empty(),
        "nothing goes to the notes folder"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
    assert_eq!(restored.recovered.len(), 1);
    assert_eq!(journal_count(&state), 0);
}

/// The same text once its file outside the notes folder is gone: the folder
/// the file left gets nothing, and the copy goes to the notes folder, named
/// for the file.
#[test]
fn removed_text_whose_file_outside_the_notes_folder_is_gone_goes_to_the_notes_folder() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let away = dir.path().join("Away");
    std::fs::create_dir_all(&away).expect("folder");
    let (id, path) = open_note_at(&state, &away.join("Away.md"), "on disk\n");
    let folder = path.parent().expect("the file's folder").to_path_buf();

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "typed away\n").expect_err("read-only");
    close_tab_handing_over(&state, &id, "typed away\n");
    set_read_only(&path, false);
    delete_buffer_inner(&state, &id).expect("remove the note from history");
    std::fs::remove_file(&path).expect("the file goes");

    let restored = relaunch(&state);
    assert!(
        std::fs::read_dir(&folder)
            .expect("read_dir")
            .next()
            .is_none(),
        "nothing is made in the folder the file left"
    );
    let copies = dated_copies(&notes, "recovered");
    assert_eq!(copies.len(), 1);
    assert!(
        copies[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Away (recovered "),
        "{copies:?}"
    );
    assert_eq!(copy_texts(&notes, "recovered"), vec!["typed away\n"]);
    assert_eq!(restored.recovered.len(), 1);
    assert_eq!(journal_count(&state), 0);
}

/// "Use the file on disk" sets the tab's text aside in a dated copy, and the
/// tab still holds that text when it closes or Writ quits. The copy holds it,
/// so the handover keeps nothing and the next launch writes no second copy
/// (ADR-033 decision 15).
#[test]
fn text_an_answer_set_aside_is_not_handed_over_again() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Answered.md"), "on disk\n");
    std::fs::write(&path, "theirs\n").expect("external write");

    let outcome = resolve_external_change_inner(&state, &id, ChangeChoice::UseDisk, "mine\n")
        .expect("the answer lands");
    assert!(outcome.conflict_copy_path.is_some());
    assert_eq!(copy_texts(&notes, "conflict"), vec!["mine\n"]);
    assert_eq!(journaled(&state, &id), None);

    record_unsaved_notes_inner(
        &state,
        vec![UnsavedNote {
            id: id.clone(),
            content: "mine\n".to_string(),
        }],
    )
    .expect("hand the text over");
    assert_eq!(
        journaled(&state, &id),
        None,
        "the copy the answer wrote holds this text"
    );

    let restored = relaunch(&state);
    assert!(restored.recovered.iter().all(|note| note.id != id));
    assert!(
        dated_copies(&notes, "recovered").is_empty(),
        "no second copy beside the note"
    );
    assert_eq!(copy_texts(&notes, "conflict"), vec!["mine\n"]);
}

/// `Keep mine` writes the tab's text the way every save does, so a write that
/// does not land leaves the text in the journal, and the next save that lands
/// clears it.
#[test]
fn keep_mine_whose_write_does_not_land_keeps_the_text() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Kept.md"), "on disk\n");
    std::fs::write(&path, "theirs\n").expect("external write");
    set_read_only(&path, true);

    let refused = resolve_external_change_inner(&state, &id, ChangeChoice::KeepMine, "mine\n")
        .expect_err("a read-only file takes no write");
    set_read_only(&path, false);
    assert!(refused.starts_with(ERR_READ_ONLY_DESTINATION), "{refused}");
    assert_eq!(journaled(&state, &id).as_deref(), Some("mine\n"));
    assert_eq!(
        copy_texts(&notes, "conflict"),
        vec!["theirs\n"],
        "what the file held went beside it first"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");

    save_buffer_content_inner(&state, &id, "mine\n").expect("the next save lands");
    assert_eq!(journaled(&state, &id), None);
}

/// Polls `ready` until it holds, for up to ten seconds.
fn wait_until(ready: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if ready() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    ready()
}

/// `Keep mine` whose own write the guard stops: the file changed again while
/// the answer ran, so the write left the tab's text in a copy beside the note.
/// The copy holds the text, so the answer keeps nothing in the journal and the
/// next launch writes no second copy.
#[test]
fn keep_mine_whose_write_the_guard_stops_keeps_nothing_a_copy_holds() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Raced.md"), "on disk\n");
    std::fs::write(&path, "theirs\n").expect("external write");

    // The answer records what it read just before its own write, so holding
    // the record stops it between the two: where a change it never read lands.
    let record = state.last_disk_hash.lock().expect("the record");
    let answered = std::thread::scope(|scope| {
        let answer = scope
            .spawn(|| resolve_external_change_inner(&state, &id, ChangeChoice::KeepMine, "mine\n"));
        let read = wait_until(|| !dated_copies(&notes, "conflict").is_empty());
        std::fs::write(&path, "theirs again\n").expect("a second external write");
        drop(record);
        let answered = answer.join().expect("the answer ran");
        assert!(read, "the answer read the file and set its text aside");
        answered
    });

    let refused = answered.expect_err("the guard stopped the write");
    assert!(refused.starts_with(ERR_FILE_CHANGED_ON_DISK), "{refused}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs again\n");
    assert_eq!(copy_texts(&notes, "conflict"), vec!["mine\n", "theirs\n"]);
    assert_eq!(
        journaled(&state, &id),
        None,
        "the copy the stopped write left holds the text"
    );

    record_unsaved_notes_inner(
        &state,
        vec![UnsavedNote {
            id: id.clone(),
            content: "mine\n".to_string(),
        }],
    )
    .expect("hand the text over");
    relaunch(&state);
    assert!(
        dated_copies(&notes, "recovered").is_empty(),
        "no second copy beside the note"
    );
}

/// A tab whose file grew past the size Writ opens keeps the text its refused
/// save left in the journal: the read that finds the file too large, the
/// launch's listing that measures it and the open that refuses it again leave
/// the entry alone, and the next launch writes the text beside the file
/// without reading it (ADR-044 §4, ADR-019).
#[test]
fn a_tab_whose_file_grew_past_the_size_writ_opens_keeps_its_journaled_text() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let notes = state.notes_root();
    let (id, path) = open_note_at(&state, &notes.join("Grew.txt"), "a line\n");

    set_read_only(&path, true);
    save_buffer_content_inner(&state, &id, "a line\ntyped\n").expect_err("read-only");
    set_read_only(&path, false);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open")
        .set_len(THRESHOLD_MAX_BYTES + 1)
        .expect("grow");

    let refused = read_buffer_content_inner(&state, &id).expect_err("too large to read");
    assert!(refused.starts_with(ERR_OPEN_TOO_LARGE), "{refused}");
    list_active_buffers_inner(&state).expect("the tabs");
    let reopened = restore_buffer_inner(&state, &id).expect("an answer");
    assert!(
        matches!(reopened.mode, FileOpenMode::Refused { .. }),
        "{:?}",
        reopened.mode
    );
    assert_eq!(journaled(&state, &id).as_deref(), Some("a line\ntyped\n"));

    let restored = relaunch(&state);
    assert_eq!(copy_texts(&notes, "recovered"), vec!["a line\ntyped\n"]);
    assert!(
        !restored.disk_states.contains_key(&id),
        "nothing was read, so nothing is recorded about the file"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        THRESHOLD_MAX_BYTES + 1
    );
    assert_eq!(journal_count(&state), 0);
}
