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
use writ_core::notes::reload::ChangeChoice;
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
    close_buffer_inner, discard_unsaved_text_inner, record_unsaved_notes_inner,
    resolve_external_change_inner, restore_note_file_inner, save_buffer_content_inner, UnsavedNote,
    ERR_FILE_CHANGED_ON_DISK, ERR_FILE_REMOVED_ON_DISK, ERR_NOTE_READ_ONLY,
    ERR_READ_ONLY_DESTINATION, ERR_WRITE_FAILED,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
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
        "INSERT INTO unsaved_text (buffer_id, path, text, recorded_at)
         VALUES (?1, NULL, CAST(X'FFFE' AS TEXT), '2026-10-13T00:00:00+00:00')",
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
