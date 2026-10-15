//! What the app does with a program's held write (ADR-053): it shows only a
//! write the settings hold for the person, answers only from the main window
//! while the write still waits, and turns an Apply into a discard for a
//! program that may no longer write.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use tempfile::TempDir;
use writ_core::config::mcp::ClientApproval;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::instance::ReviewAsk;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_core::write_review::ReviewAnswer;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_storage::write_review::take_answer;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;
use writ_tauri_lib::write_review::{
    answer_inner, route_review, ReviewAnswerError, ReviewAsks, ReviewRoute,
};

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

fn set(state: &AppState, program: &str, write: bool, review: bool) {
    let mut config = state.config.lock().expect("config");
    config.mcp.enabled = true;
    config.mcp.approved_clients = vec![ClientApproval {
        name: program.to_string(),
        first_seen: chrono::Utc::now(),
        read: true,
        write,
        context: false,
        review,
    }];
}

fn ask(token: &str, program: &str) -> ReviewAsk {
    ReviewAsk {
        token: token.to_string(),
        program: program.to_string(),
    }
}

#[test]
fn a_held_write_is_shown_only_for_a_program_set_to_ask_first() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    state.frontend_ready.store(true, Ordering::SeqCst);

    assert_eq!(
        route_review(&state, &ask("r-1", "Claude Code")),
        ReviewRoute::NotHeld
    );

    set(&state, "Claude Code", true, false);
    assert_eq!(
        route_review(&state, &ask("r-1", "Claude Code")),
        ReviewRoute::NotHeld
    );

    set(&state, "Claude Code", false, true);
    assert_eq!(
        route_review(&state, &ask("r-1", "Claude Code")),
        ReviewRoute::NotHeld,
        "a program that cannot write has nothing to hold"
    );

    set(&state, "Claude Code", true, true);
    assert_eq!(
        route_review(&state, &ask("r-1", "Claude Code")),
        ReviewRoute::Show
    );
    assert_eq!(
        route_review(&state, &ask("r-1", "Codex")),
        ReviewRoute::NotHeld
    );
    assert_eq!(
        route_review(&state, &ask("../config", "Claude Code")),
        ReviewRoute::NotHeld
    );

    state.frontend_ready.store(false, Ordering::SeqCst);
    assert_eq!(
        route_review(&state, &ask("r-1", "Claude Code")),
        ReviewRoute::NotReady
    );

    state.frontend_ready.store(true, Ordering::SeqCst);
    state.config.lock().unwrap().mcp.enabled = false;
    assert_eq!(
        route_review(&state, &ask("r-1", "Claude Code")),
        ReviewRoute::NotHeld
    );
}

#[test]
fn only_the_main_window_answers_and_only_a_write_still_waiting() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    set(&state, "Claude Code", true, true);
    let asks = ReviewAsks::default();
    asks.hold(
        "r-2",
        "Claude Code",
        writ_core::write_review::REVIEW_WAIT,
        Instant::now(),
    );

    assert_eq!(
        answer_inner(&state, &asks, "capture", "r-2", true),
        Err(ReviewAnswerError::NotMainWindow)
    );
    assert_eq!(
        answer_inner(&state, &asks, "main", "r-9", true),
        Err(ReviewAnswerError::NotAsked)
    );
    assert_eq!(take_answer(&state.writ_dir, "r-2"), None);

    answer_inner(&state, &asks, "main", "r-2", true).expect("applied");
    assert_eq!(
        take_answer(&state.writ_dir, "r-2"),
        Some(ReviewAnswer::Apply)
    );
    assert_eq!(
        answer_inner(&state, &asks, "main", "r-2", true),
        Err(ReviewAnswerError::NotAsked),
        "answered once"
    );

    asks.hold(
        "r-3",
        "Claude Code",
        writ_core::write_review::REVIEW_WAIT,
        Instant::now(),
    );
    answer_inner(&state, &asks, "main", "r-3", false).expect("discarded");
    assert_eq!(
        take_answer(&state.writ_dir, "r-3"),
        Some(ReviewAnswer::Discard)
    );
}

#[test]
fn an_apply_for_a_program_that_may_no_longer_write_is_a_discard() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    set(&state, "Claude Code", true, true);
    let asks = ReviewAsks::default();
    asks.hold(
        "r-4",
        "Claude Code",
        writ_core::write_review::REVIEW_WAIT,
        Instant::now(),
    );

    set(&state, "Claude Code", false, true);
    answer_inner(&state, &asks, "main", "r-4", true).expect("answered");
    assert_eq!(
        take_answer(&state.writ_dir, "r-4"),
        Some(ReviewAnswer::Discard)
    );
}
