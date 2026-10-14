//! The heartbeat against a file whose reads never return.
//!
//! A tab open on a network share whose server went away holds a file that
//! nothing can read: a read blocks until the share answers. A FIFO with no
//! writer is the same file on any Unix machine, which is what lets the stall
//! be built here rather than waited for.
//!
//! Every save takes the store, and the save commands run on the main thread,
//! so a heartbeat that holds the store across that read freezes the window
//! until the share comes back.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
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
use writ_tauri_lib::heartbeat;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

/// Long enough for a beat and a save on an idle machine many times over, and
/// short enough that a stalled run fails rather than hangs the suite.
const DEADLINE: Duration = Duration::from_secs(5);

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
    }
}

/// A file whose reads block until something writes to it, which nothing will.
fn stalled_file(dir: &Path) -> PathBuf {
    let share = dir.join("share");
    std::fs::create_dir_all(&share).expect("share folder");
    let path = share.join("Stalled.md");
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo failed for {}", path.display());
    path
}

/// The row of a tab open on the stalled file, inserted directly: opening it
/// through the command layer would read it, and the read would block there.
fn tab_on(id: &str, path: &Path) -> BufferDocument {
    let now = chrono::Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: "Stalled".to_string(),
        filename: format!("{id}.md"),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(path.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
        encoding: None,
    }
}

/// Opens a note in the notes folder the way the frontend reaches one.
fn open_note(state: &AppState, name: &str, content: &str) -> String {
    let path = state.notes_root().join(name);
    std::fs::write(&path, content).expect("write");
    let canonical = canonicalize_for_authorization(&path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id
}

#[test]
fn a_save_completes_while_a_heartbeat_read_is_hung() {
    let dir = TempDir::new().expect("temp dir");
    let state = Arc::new(make_state(&dir));

    let stalled = stalled_file(dir.path());
    state
        .store
        .lock()
        .expect("store")
        .insert(&tab_on("stalled", &stalled))
        .expect("insert the stalled tab");
    let typing = open_note(&state, "Typing.md", "first line\n");

    let (beat_done, beat_finished) = mpsc::channel();
    let beat_state = Arc::clone(&state);
    std::thread::spawn(move || {
        heartbeat::beat(&beat_state);
        let _ = beat_done.send(());
    });
    let beat_returned_on_its_own = beat_finished.recv_timeout(DEADLINE).is_ok();
    let store_held_by_the_beat = !beat_returned_on_its_own
        && matches!(
            state.store.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        );

    let (save_done, save_finished) = mpsc::channel();
    let save_state = Arc::clone(&state);
    let typing_id = typing.clone();
    std::thread::spawn(move || {
        let outcome = save_buffer_content_inner(&save_state, &typing_id, "typed meanwhile\n");
        let _ = save_done.send(outcome);
    });
    let saved = save_finished.recv_timeout(DEADLINE);

    // Lets a stalled beat go, so the run ends either way. Only a beat that is
    // still waiting is reading the file: opening the write end with nobody
    // reading would block this thread instead.
    if !beat_returned_on_its_own {
        drop(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&stalled)
                .expect("open the write end"),
        );
        let _ = beat_finished.recv_timeout(DEADLINE);
        let _ = save_finished.recv_timeout(DEADLINE);
    }

    let save_waited = saved.is_err();
    assert!(
        beat_returned_on_its_own,
        "the heartbeat read a tab's file and waited on it \
         (holding the store: {store_held_by_the_beat}; a save waited behind it: {save_waited})"
    );
    let saved = saved.expect("a save waited behind the heartbeat");
    assert!(saved.is_ok(), "the save failed: {saved:?}");
    assert_eq!(
        std::fs::read_to_string(state.notes_root().join("Typing.md")).expect("read"),
        "typed meanwhile\n"
    );
}
