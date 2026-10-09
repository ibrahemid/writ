//! The shutdown a restart or an update install runs before the process goes.
//!
//! "Restart to update" used to restart without asking the window to write
//! what was typed inside the autosave debounce, and on Windows the installer
//! ends the process from inside the updater plugin. Both now run the quit's
//! handshake first: ask the frontend to flush, wait for its answer, then write
//! what Rust holds. The shutdown work itself (`finish_shutdown`) needs a live
//! `AppHandle`, so it is passed in, and these pin the order around it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::preview::ContentRendererRegistry;
use writ_core::recovery::QUIT_FLUSH_TIMEOUT;
use writ_core::update::UpdatePhase;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::{QuitDecision, QuitState};
use writ_tauri_lib::relaunch::{shut_down_for_relaunch, RelaunchShutdown};
use writ_tauri_lib::security::AuthorizedPaths;
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

/// Answers the flush request the way the webview does: the editors write,
/// then `confirm_quit_flush` arrives, a moment later and from another thread.
fn answer_flush_requests(state: &AppState, log: Arc<Mutex<Vec<&'static str>>>) {
    let quit = state.quit.clone();
    state.event_bus.subscribe(move |event| {
        if !matches!(event, WritEvent::FlushBeforeQuit) {
            return;
        }
        let quit = quit.clone();
        let log = log.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            log.lock().unwrap().push("editors flushed");
            quit.confirm_flush();
        });
    });
}

#[test]
fn a_relaunch_flushes_the_editors_before_the_shutdown_work() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    state.frontend_ready.store(true, Ordering::SeqCst);
    let log = Arc::new(Mutex::new(Vec::new()));
    answer_flush_requests(&state, log.clone());

    let started = Instant::now();
    let outcome = shut_down_for_relaunch(&state, || {
        log.lock().unwrap().push("finish_shutdown");
    });

    assert_eq!(outcome, RelaunchShutdown::Finished);
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["editors flushed", "finish_shutdown"],
        "the shutdown snapshot was taken before the editors had written"
    );
    assert!(
        started.elapsed() < QUIT_FLUSH_TIMEOUT,
        "the relaunch sat through the timeout after the answer arrived"
    );
    assert!(state.quit.is_complete());
    assert!(
        state.notes_index_cancel.load(Ordering::SeqCst),
        "the reconcile walk is left running into the relaunch"
    );
}

#[test]
fn a_relaunch_leaves_nothing_for_the_exit_path_to_redo() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    state.frontend_ready.store(true, Ordering::SeqCst);
    answer_flush_requests(&state, Arc::new(Mutex::new(Vec::new())));

    shut_down_for_relaunch(&state, || {});

    // The restart raises `ExitRequested` and `Exit` after this; both have to
    // find the work done rather than snapshot a second time.
    assert!(!state.quit.claim_final_shutdown());
    assert_eq!(
        state.quit.begin(Some(tauri::RESTART_EXIT_CODE)),
        QuitDecision::Proceed
    );
}

#[test]
fn a_relaunch_before_the_window_is_ready_shuts_down_without_asking() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let asked = Arc::new(AtomicBool::new(false));
    let seen = asked.clone();
    state.event_bus.subscribe(move |event| {
        if matches!(event, WritEvent::FlushBeforeQuit) {
            seen.store(true, Ordering::SeqCst);
        }
    });
    let ran = AtomicBool::new(false);

    let started = Instant::now();
    let outcome = shut_down_for_relaunch(&state, || ran.store(true, Ordering::SeqCst));

    assert_eq!(outcome, RelaunchShutdown::Finished);
    assert!(ran.load(Ordering::SeqCst), "the shutdown work did not run");
    assert!(
        !asked.load(Ordering::SeqCst),
        "a window with nothing loaded was asked to flush"
    );
    assert!(started.elapsed() < QUIT_FLUSH_TIMEOUT);
}

#[test]
fn a_relaunch_whose_window_never_answers_still_shuts_down() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    state.frontend_ready.store(true, Ordering::SeqCst);
    let ran = AtomicBool::new(false);

    let started = Instant::now();
    let outcome = shut_down_for_relaunch(&state, || ran.store(true, Ordering::SeqCst));

    assert_eq!(outcome, RelaunchShutdown::Finished);
    assert!(ran.load(Ordering::SeqCst));
    let waited = started.elapsed();
    assert!(waited >= QUIT_FLUSH_TIMEOUT, "gave up early: {waited:?}");
    assert!(
        waited < QUIT_FLUSH_TIMEOUT * 2,
        "hung on the window: {waited:?}"
    );
}

#[test]
fn a_relaunch_during_a_quit_waits_for_that_quit_and_writes_nothing_twice() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    state.frontend_ready.store(true, Ordering::SeqCst);
    assert_eq!(state.quit.begin(None), QuitDecision::StartFlush);
    let quit = state.quit.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        quit.finish();
    });
    let ran = AtomicBool::new(false);

    let started = Instant::now();
    let outcome = shut_down_for_relaunch(&state, || ran.store(true, Ordering::SeqCst));

    assert_eq!(outcome, RelaunchShutdown::AlreadyLeaving);
    assert!(
        !ran.load(Ordering::SeqCst),
        "the shutdown work ran a second time"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(80),
        "returned while the quit was still writing; the installer would cut its snapshot"
    );
}

#[test]
fn a_second_relaunch_after_the_first_does_nothing() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    shut_down_for_relaunch(&state, || {});
    let ran = AtomicBool::new(false);

    let outcome = shut_down_for_relaunch(&state, || ran.store(true, Ordering::SeqCst));

    assert_eq!(outcome, RelaunchShutdown::AlreadyLeaving);
    assert!(!ran.load(Ordering::SeqCst));
}
