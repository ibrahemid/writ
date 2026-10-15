//! What the app does with a `get_context` ask (ADR-053): it answers only for a
//! program the settings let see the open file, only from the main window, and
//! names the file from its own record of the tab.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::mcp::ClientApproval;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::instance::ContextAsk;
use writ_core::open_context::{OpenContext, Selected, TextPoint};
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
use writ_storage::open_context::take_answer;
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::open_context::{
    answer_inner, resolve_answer, route_ask, AskRoute, ContextAnswerError, ContextAsks,
    WindowContext,
};
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

fn open(state: &AppState, path: &std::path::Path) -> String {
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("an opened file has a row")
        .id
}

fn allow(state: &AppState, program: &str, context: bool) {
    let mut config = state.config.lock().expect("config");
    config.mcp.enabled = true;
    config.mcp.approved_clients = vec![ClientApproval {
        name: program.to_string(),
        first_seen: chrono::Utc::now(),
        read: true,
        write: true,
        context,
        review: false,
    }];
}

fn ask(token: &str, program: &str) -> ContextAsk {
    ContextAsk {
        token: token.to_string(),
        program: program.to_string(),
    }
}

fn selection() -> Selected {
    Selected {
        text: "launch the site".to_string(),
        from: TextPoint { line: 2, column: 1 },
        to: TextPoint {
            line: 2,
            column: 16,
        },
    }
}

#[test]
fn an_ask_is_answered_only_for_a_program_allowed_to_see_the_open_file() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    state.frontend_ready.store(true, Ordering::SeqCst);

    assert_eq!(
        route_ask(&state, &ask("t-1", "Claude Code")),
        AskRoute::Refused
    );

    allow(&state, "Claude Code", false);
    assert_eq!(
        route_ask(&state, &ask("t-1", "Claude Code")),
        AskRoute::Refused,
        "reading and writing do not carry it"
    );

    allow(&state, "Claude Code", true);
    assert_eq!(
        route_ask(&state, &ask("t-1", "Claude Code")),
        AskRoute::AskWindow
    );
    assert_eq!(route_ask(&state, &ask("t-1", "Codex")), AskRoute::Refused);
    assert_eq!(
        route_ask(&state, &ask("../config", "Claude Code")),
        AskRoute::Refused
    );

    state.config.lock().unwrap().mcp.enabled = false;
    assert_eq!(
        route_ask(&state, &ask("t-1", "Claude Code")),
        AskRoute::Refused
    );
}

#[test]
fn a_window_that_is_not_ready_answers_that_no_file_is_open() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    allow(&state, "Claude Code", true);
    assert_eq!(
        route_ask(&state, &ask("t-1", "Claude Code")),
        AskRoute::NothingOpen
    );
}

#[test]
fn the_file_is_named_from_the_apps_record_of_the_tab() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("Plan.md");
    std::fs::write(&file, "# Plan\nlaunch the site\n").unwrap();
    let id = open(&state, &file);

    let answer = resolve_answer(
        &state,
        WindowContext {
            buffer_id: Some(id),
            selections: vec![selection()],
            cursor: Some(TextPoint {
                line: 2,
                column: 16,
            }),
            unsaved: true,
            truncated: false,
        },
    );
    let named = canonicalize_for_authorization(&file).unwrap();
    assert_eq!(answer.path.as_deref(), Some(named.as_str()));
    assert_eq!(answer.selections, vec![selection()]);
    assert!(answer.unsaved);

    let unknown = resolve_answer(
        &state,
        WindowContext {
            buffer_id: Some("not-a-tab".to_string()),
            selections: vec![selection()],
            cursor: None,
            unsaved: true,
            truncated: true,
        },
    );
    assert_eq!(unknown, OpenContext::nothing_open());
}

#[test]
fn only_the_main_window_answers_and_only_an_ask_still_waited_on() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let asks = ContextAsks::default();
    asks.hold("t-2", std::time::Instant::now());

    assert_eq!(
        answer_inner(&state, &asks, "capture", "t-2", WindowContext::default()),
        Err(ContextAnswerError::NotMainWindow)
    );
    assert_eq!(
        answer_inner(&state, &asks, "main", "t-9", WindowContext::default()),
        Err(ContextAnswerError::NotAsked)
    );
    assert_eq!(take_answer(&state.writ_dir, "t-2"), None);

    answer_inner(&state, &asks, "main", "t-2", WindowContext::default()).expect("answered");
    assert_eq!(
        take_answer(&state.writ_dir, "t-2"),
        Some(OpenContext::nothing_open())
    );
    assert_eq!(
        answer_inner(&state, &asks, "main", "t-2", WindowContext::default()),
        Err(ContextAnswerError::NotAsked),
        "an ask is answered once"
    );
}
