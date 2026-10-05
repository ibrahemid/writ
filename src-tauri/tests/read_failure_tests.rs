//! A read that fails reaches the editor as a stable code, never as an empty
//! note.
//!
//! The editor decides from the code alone whether it may mount the file: a
//! rejection it cannot tell apart from "the file is empty" is how a file that
//! is not UTF-8 opened as a blank page whose first keystroke replaced it.
//! These go through the real open and read commands, because the failure is
//! the composition of the two: the open records the file and succeeds, and
//! only the read finds the bytes it cannot hand over.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::file_ops::FileOpenMode;
use writ_core::notes::line_ending::LineEnding;
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
    close_buffer_inner, read_buffer_content_inner, save_buffer_content_inner,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
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

    let note_history = Arc::new(
        writ_storage::note_history::NoteHistoryStore::open(&writ_dir).expect("version store"),
    );

    AppState {
        store: Mutex::new(store),
        config_store,
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
        unsaved_on_exit: Mutex::new(std::collections::HashMap::new()),
    }
}

/// Opens `path`, taking the single-use authorization the gate wants.
fn open(state: &AppState, path: &Path) -> String {
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id
}

fn read_error(state: &AppState, id: &str) -> String {
    match read_buffer_content_inner(state, id) {
        Ok(bytes) => panic!(
            "the read handed over {} bytes instead of failing",
            bytes.len()
        ),
        Err(message) => message,
    }
}

#[test]
fn a_file_that_is_no_longer_utf8_fails_its_read_with_a_code() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("prices.csv");
    std::fs::write(&file, "cafe,3.50\n").unwrap();
    let id = open(&state, &file);

    // Another program saves it as Windows-1252, where 0xE9 is "é" and on its
    // own is not UTF-8. The tab is still open and reads it on the next switch
    // or launch.
    let cp1252: &[u8] = b"caf\xe9,3.50\r\nth\xe9,2.00\r\n";
    std::fs::write(&file, cp1252).unwrap();
    let error = read_error(&state, &id);

    assert!(
        error.starts_with("ERR_READ_NOT_UTF8:"),
        "the editor cannot tell this read from an empty file: {error}"
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        cp1252,
        "a failed read must leave the file as it was"
    );
}

/// The line ending the note's row holds.
fn line_ending_of(state: &AppState, id: &str) -> LineEnding {
    state.store.lock().unwrap().get(id).unwrap().line_ending
}

#[test]
fn a_file_that_is_not_utf8_opens_into_a_read_that_fails_with_a_code() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("prices-1252.csv");
    let cp1252: &[u8] = b"caf\xe9,3.50\r\nth\xe9,2.00\r\n";
    std::fs::write(&file, cp1252).unwrap();

    // An open that failed had nowhere to say why: the click in the file tree
    // did nothing. It opens a tab, and the tab's read carries the reason.
    let canonical = canonicalize_for_authorization(&file).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("a file that is not UTF-8 opens");
    assert!(
        matches!(opened.mode, FileOpenMode::Normal),
        "a text file that is not UTF-8 is not a binary or large file"
    );
    let id = opened.doc.expect("the open carries its note").id;
    let error = read_error(&state, &id);

    assert!(
        error.starts_with("ERR_READ_NOT_UTF8:"),
        "the editor cannot tell this read from an empty file: {error}"
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        cp1252,
        "opening and reading must leave the file as it was"
    );
}

#[test]
fn a_closed_file_reopened_after_it_stopped_being_utf8_fails_its_read_with_a_code() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("menu.txt");
    std::fs::write(&file, "café, in UTF-8\n").unwrap();
    let id = open(&state, &file);
    close_buffer_inner(&state, &id).expect("close");

    let cp1252: &[u8] = b"caf\xe9, in Windows-1252\r\n";
    std::fs::write(&file, cp1252).unwrap();
    let reopened = open(&state, &file);
    let error = read_error(&state, &reopened);

    assert_eq!(reopened, id, "the closed note comes back, not a second one");
    assert!(
        error.starts_with("ERR_READ_NOT_UTF8:"),
        "a reopened file that is not UTF-8 read as an uncoded failure: {error}"
    );
    assert_eq!(line_ending_of(&state, &id), LineEnding::CrLf);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        cp1252,
        "reopening and reading must leave the file as it was"
    );
}

#[test]
fn a_file_converted_to_utf8_after_its_failed_read_reads_and_saves_with_its_own_ending() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("prices-1252.csv");
    std::fs::write(&file, b"caf\xe9,3.50\r\nth\xe9,2.00\r\n").unwrap();
    let id = open(&state, &file);
    assert_eq!(
        line_ending_of(&state, &id),
        LineEnding::CrLf,
        "the ending is counted from the bytes even when they are not UTF-8"
    );
    read_error(&state, &id);

    // The person converts it outside Writ and presses Try again.
    std::fs::write(&file, "café,3.50\r\nthé,2.00\r\n").unwrap();
    let bytes = read_buffer_content_inner(&state, &id).expect("a converted file reads");
    assert_eq!(bytes, "café,3.50\r\nthé,2.00\r\n".as_bytes());

    save_buffer_content_inner(&state, &id, "café,3.75\nthé,2.00\n")
        .expect("the file opened from a failed read can be saved once it reads");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "café,3.75\r\nthé,2.00\r\n"
    );
}

#[test]
fn a_file_that_went_away_fails_its_read_with_a_code() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("gone.txt");
    std::fs::write(&file, "here when it was opened\n").unwrap();

    let id = open(&state, &file);
    std::fs::remove_file(&file).unwrap();
    let error = read_error(&state, &id);

    assert!(
        error.starts_with("ERR_READ_FILE_MISSING:"),
        "a missing file read as an uncoded failure: {error}"
    );
}

#[cfg(unix)]
#[test]
fn a_file_writ_may_not_read_fails_its_read_with_a_code() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("locked.txt");
    std::fs::write(&file, "somebody else's\n").unwrap();

    let id = open(&state, &file);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    // A process that may read anything (root in a container) reads it anyway,
    // and there is no refusal to check.
    if std::fs::read(&file).is_ok() {
        return;
    }
    let error = read_error(&state, &id);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(
        error.starts_with("ERR_READ_PERMISSION_DENIED:"),
        "a refused read reached the editor without its code: {error}"
    );
}

#[test]
fn a_utf8_file_still_reads() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("plain.txt");
    std::fs::write(&file, "café, in UTF-8\n").unwrap();

    let id = open(&state, &file);
    let bytes = read_buffer_content_inner(&state, &id).expect("a UTF-8 file reads");

    assert_eq!(bytes, "café, in UTF-8\n".as_bytes());
}

#[test]
fn a_binary_file_still_opens_as_its_hex_view() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("blob.bin");
    std::fs::write(&file, b"\x00\x01\x02\xff\xfe binary").unwrap();

    let id = open(&state, &file);
    let bytes = read_buffer_content_inner(&state, &id).expect("a binary file reads read-only");

    assert!(!bytes.is_empty(), "the hex view came back empty");
}
