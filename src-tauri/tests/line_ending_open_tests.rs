//! The line ending a note carries is read off its file by the open path.
//!
//! Every one of these goes through a real command rather than building a
//! `BufferDocument` with the ending already set: the detection lives in
//! `commands::file` and `commands::buffer`, so a test that hands the store a
//! finished row proves nothing about it.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::notes::line_ending::LineEnding;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
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
        notes_index: Arc::new(NotesIndexStore::open(&db_path).expect("notes index db")),
        notes_index_cancel: Arc::new(AtomicBool::new(false)),
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

fn recorded_ending(state: &AppState, id: &str) -> LineEnding {
    state
        .store
        .lock()
        .expect("store")
        .get(id)
        .expect("get")
        .line_ending
}

#[test]
fn a_windows_file_records_its_ending_when_it_is_first_opened() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("windows.md");
    std::fs::write(&file, "alpha\r\nbeta\r\n").unwrap();

    let id = open(&state, &file);

    assert_eq!(
        recorded_ending(&state, &id),
        LineEnding::CrLf,
        "the first open did not read the file's ending"
    );
}

#[test]
fn reopening_a_closed_tab_reads_the_files_ending_again() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("notes.md");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();

    let id = open(&state, &file);
    assert_eq!(recorded_ending(&state, &id), LineEnding::Lf);
    close_buffer_inner(&state, &id).expect("close");

    // A checkout with core.autocrlf, or the same note edited on a Windows
    // machine, while the tab was closed.
    std::fs::write(&file, "alpha\r\nbeta\r\n").unwrap();
    let reopened = open(&state, &file);

    assert_eq!(reopened, id, "the same row should have come back");
    assert_eq!(
        recorded_ending(&state, &id),
        LineEnding::CrLf,
        "reopening the tab did not read the file again"
    );
}

#[test]
fn opening_a_note_that_is_already_open_follows_the_file() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("notes.md");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();

    let id = open(&state, &file);
    std::fs::write(&file, "alpha\r\nbeta\r\n").unwrap();
    let again = open(&state, &file);

    assert_eq!(again, id, "the open tab should have been reused");
    assert_eq!(
        recorded_ending(&state, &id),
        LineEnding::CrLf,
        "reopening an open tab did not resync the file's ending"
    );
}

#[test]
fn a_reload_follows_a_file_that_gained_carriage_returns() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("notes.md");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();

    let id = open(&state, &file);
    assert_eq!(recorded_ending(&state, &id), LineEnding::Lf);

    // Something outside Writ rewrote the file while the tab was open, and the
    // user said yes to the reload.
    std::fs::write(&file, "alpha\r\nbeta\r\n").unwrap();
    read_buffer_content_inner(&state, &id).expect("reload");

    assert_eq!(
        recorded_ending(&state, &id),
        LineEnding::CrLf,
        "the reload did not follow the file's ending"
    );

    // What the row is for: the next save keeps the convention the file gained
    // instead of stripping every carriage return back out of it.
    save_buffer_content_inner(&state, &id, "alpha\nBETA\n").expect("save");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"alpha\r\nBETA\r\n",
        "the save wrote the ending the row held before the reload"
    );
}
