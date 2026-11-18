//! The claim that decides who shows the window Jotra starts hidden.
//!
//! The frontend signals its first paint and a timer stands behind it, so both
//! reach the window on a slow launch. These pin what each of them comes away
//! with.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use jotra_core::config::JotraConfig;
use jotra_core::events::bus::EventBus;
use jotra_core::preview::ContentRendererRegistry;
use jotra_core::update::UpdatePhase;
use jotra_core::watcher::reconcile::ReconcileGate;
use jotra_plugin::transform::TransformRegistry;
use jotra_storage::buffer_store::BufferStore;
use jotra_storage::config_store::ConfigStore;
use jotra_storage::database::connection::open_database;
use jotra_storage::database::migrations::run_migrations;
use jotra_storage::layout_state::LayoutStateStore;
use jotra_storage::notes_index::NotesIndexStore;
use jotra_tauri_lib::claim_reveal;
use jotra_tauri_lib::preview::handler::RenderCache;
use jotra_tauri_lib::quit::QuitState;
use jotra_tauri_lib::security::AuthorizedPaths;
use jotra_tauri_lib::state::AppState;
use jotra_tauri_lib::watcher::handler::create_ignore_set;
use jotra_tauri_lib::window_state::RevealAction;
use tempfile::TempDir;

fn make_state(dir: &TempDir) -> AppState {
    let jotra_dir = dir.path().to_path_buf();
    let buffers_dir = jotra_dir.join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");

    let notes_root = jotra_dir.join("Jotra");
    std::fs::create_dir_all(&notes_root).expect("notes folder");
    let notes_root = jotra_tauri_lib::security::canonicalize_root(&notes_root).expect("canonical");

    let db_path = jotra_dir.join("jotra.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let store = BufferStore::new(conn, buffers_dir.clone());

    let config_path = jotra_dir.join("config.toml");
    let config_store = ConfigStore::new(config_path);

    let note_history = Arc::new(
        jotra_storage::note_history::NoteHistoryStore::open(&jotra_dir).expect("version store"),
    );

    AppState {
        store: Mutex::new(store),
        config_store,
        config: Mutex::new(JotraConfig::default()),
        jotra_dir,
        buffers_dir,
        notes_root: RwLock::new(notes_root),
        first_run: false,
        first_run_finished: std::sync::atomic::AtomicBool::new(false),
        menu_apps: std::sync::Mutex::new(None),
        retitle_watch: std::sync::Arc::new(jotra_tauri_lib::first_run::RetitleWatch::new()),
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
        command_usage: jotra_storage::command_usage::CommandUsageStore::new(
            open_database(&db_path).expect("usage db"),
        ),
        config_diagnostics: std::sync::Mutex::new(Vec::new()),
        recovered_buffers: Mutex::new(Vec::new()),
        was_dirty_shutdown: false,
        workspace_root: Mutex::new(None),
        workspace_watcher: Mutex::new(None),
        inbox_root: Mutex::new(None),
        inbox_watcher: Mutex::new(None),
        fts_scheduler: jotra_tauri_lib::fts_scheduler::FtsScheduler::new(),
        workspace_index: Arc::new(RwLock::new(
            jotra_tauri_lib::workspace_index::WorkspaceIndex::new(None),
        )),
        search_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        last_disk_hash: Mutex::new(std::collections::HashMap::new()),
        source_records: Mutex::new(std::collections::HashMap::new()),
        buffer_writes: Default::default(),
    }
}

// The frontend's signal arrives first on a healthy launch and shows the
// window; the timer behind it finds the window up and stands down.
#[test]
fn the_timer_stands_down_behind_a_window_that_is_up() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);

    assert_eq!(claim_reveal(&state, || Ok(false)), RevealAction::Show);
    assert_eq!(claim_reveal(&state, || Ok(true)), RevealAction::Skip);
}

// The launch this exists for: the show was taken and the window is not there.
// A claim nobody can see is worth nothing, so the timer shows it again.
#[test]
fn the_timer_shows_again_when_the_claimed_window_is_not_up() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);

    assert_eq!(claim_reveal(&state, || Ok(false)), RevealAction::Show);
    assert_eq!(claim_reveal(&state, || Ok(false)), RevealAction::Show);
}

// A visibility read that failed used to count as "already up", which is how a
// launch ended with a running app, no window, and nothing in the log.
#[test]
fn a_failed_visibility_read_does_not_stand_the_timer_down() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);

    assert_eq!(claim_reveal(&state, || Ok(false)), RevealAction::Show);
    assert_eq!(claim_reveal(&state, || Err(())), RevealAction::Show);
}

// Hiding Jotra in the seconds after opening it is still hiding it.
#[test]
fn a_window_the_user_put_away_stays_away() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);

    assert_eq!(claim_reveal(&state, || Ok(false)), RevealAction::Show);
    state
        .window_dismissed
        .store(true, std::sync::atomic::Ordering::SeqCst);

    assert_eq!(claim_reveal(&state, || Ok(false)), RevealAction::Skip);
}
