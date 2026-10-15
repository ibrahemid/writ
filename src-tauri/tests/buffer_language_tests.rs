//! The language chosen for a file with Change Language.
//!
//! An open records no language: the editor reads the file's name every time.
//! `set_buffer_language` records a choice, clears it with `None`, refuses an
//! id no picker could have sent, and the choice stays with the file across a
//! close and reopen.

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
use writ_tauri_lib::commands::buffer::{close_buffer_inner, set_buffer_language_inner};
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
        pending_lines: Mutex::new(Default::default()),
        cli_waits: Mutex::new(Default::default()),
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
        command_usage: writ_storage::command_usage::CommandUsageStore::new(
            open_database(&db_path).expect("usage db"),
        ),
        config_diagnostics: std::sync::Mutex::new(Vec::new()),
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
        buffer_writes: Default::default(),
    }
}

fn open(state: &AppState, path: &std::path::Path) -> writ_core::buffer::document::BufferDocument {
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("an opened file has a row")
}

#[test]
fn an_opened_file_records_no_language() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("page.tsx");
    std::fs::write(&file, "export const A = () => <div />;\n").unwrap();

    let doc = open(&state, &file);

    assert_eq!(doc.language, None);
    let stored = state.store.lock().unwrap().get(&doc.id).unwrap();
    assert_eq!(stored.language, None);
}

#[test]
fn a_chosen_language_is_recorded_and_answered() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("snippet.txt");
    std::fs::write(&file, "print('hi')\n").unwrap();
    let doc = open(&state, &file);

    let answered = set_buffer_language_inner(&state, &doc.id, Some("python")).unwrap();

    assert_eq!(answered.id, doc.id);
    assert_eq!(answered.language.as_deref(), Some("python"));
    let stored = state.store.lock().unwrap().get(&doc.id).unwrap();
    assert_eq!(stored.language.as_deref(), Some("python"));
}

#[test]
fn clearing_the_choice_hands_the_file_back_to_its_name() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let doc = open(&state, &file);
    set_buffer_language_inner(&state, &doc.id, Some("plaintext")).unwrap();

    let answered = set_buffer_language_inner(&state, &doc.id, None).unwrap();

    assert_eq!(answered.language, None);
}

#[test]
fn the_choice_stays_with_the_file_across_a_close_and_reopen() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("settings.conf");
    std::fs::write(&file, "key = 1\n").unwrap();
    let doc = open(&state, &file);
    set_buffer_language_inner(&state, &doc.id, Some("toml")).unwrap();
    close_buffer_inner(&state, &doc.id).unwrap();

    let reopened = open(&state, &file);

    assert_eq!(reopened.id, doc.id);
    assert_eq!(reopened.language.as_deref(), Some("toml"));
}

#[test]
fn an_id_no_picker_could_send_is_refused_and_nothing_is_recorded() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("notes.txt");
    std::fs::write(&file, "text\n").unwrap();
    let doc = open(&state, &file);

    for bad in ["", " python", "py\u{7}thon", &"a".repeat(65)] {
        let error = set_buffer_language_inner(&state, &doc.id, Some(bad)).unwrap_err();
        assert!(
            error.starts_with("invalid language: "),
            "{bad:?} gave {error}"
        );
    }

    let stored = state.store.lock().unwrap().get(&doc.id).unwrap();
    assert_eq!(stored.language, None);
}

#[test]
fn a_buffer_that_does_not_exist_is_an_error() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    assert!(set_buffer_language_inner(&state, "no-such-buffer", Some("python")).is_err());
}
