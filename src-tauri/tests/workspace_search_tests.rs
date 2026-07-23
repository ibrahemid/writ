//! IPC coverage for the workspace search commands (ADR-026, C2/C3):
//! `search_workspace_files`, `workspace_index_status`, and the content-search
//! engine behind `search_workspace_content` (via its Tauri-free core
//! `run_content_search`), including streamed batch delivery and generation
//! staleness.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::workspace_grep::GrepLimits;
use writ_tauri_lib::commands::workspace::{
    run_content_search, search_workspace_files_inner, workspace_index_status_inner, SearchBatch,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;
use writ_tauri_lib::workspace_index::{self, WorkspaceIndex};

fn write_file(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn make_state(writ_dir_holder: &TempDir, ws_root: Option<PathBuf>) -> AppState {
    let writ_dir = writ_dir_holder.path().to_path_buf();
    let buffers_dir = writ_dir.join("buffers");
    std::fs::create_dir_all(&buffers_dir).unwrap();

    let db_path = writ_dir.join("writ.db");
    let conn = open_database(&db_path).unwrap();
    run_migrations(&conn).unwrap();
    let store = BufferStore::new(conn, buffers_dir.clone());

    let config_path = writ_dir.join("config.toml");
    let config_store = ConfigStore::new(config_path);

    AppState {
        store: Mutex::new(store),
        config_store,
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        pending_opens: Mutex::new(Vec::new()),
        frontend_ready: AtomicBool::new(false),
        transforms: RwLock::new(TransformRegistry::new()),
        event_bus: Arc::new(EventBus::new()),
        update_phase: Mutex::new(UpdatePhase::default()),
        authorized_paths: AuthorizedPaths::new(),
        preview_registry: Arc::new(RwLock::new(ContentRendererRegistry::new())),
        preview_render_cache: Arc::new(RenderCache::new()),
        layout_state: LayoutStateStore::new(open_database(&db_path).unwrap()),
        recovered_buffers: Mutex::new(Vec::new()),
        was_dirty_shutdown: false,
        workspace_root: Mutex::new(ws_root.clone()),
        workspace_watcher: Mutex::new(None),
        inbox_root: Mutex::new(None),
        inbox_watcher: Mutex::new(None),
        fts_scheduler: writ_tauri_lib::fts_scheduler::FtsScheduler::new(),
        workspace_index: Arc::new(RwLock::new(WorkspaceIndex::new(ws_root))),
        search_generation: Arc::new(AtomicU64::new(0)),
    }
}

#[test]
fn search_workspace_files_ranks_index_and_reports_status() {
    let writ_dir = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    write_file(ws.path(), "src/main.rs", "x");
    write_file(ws.path(), "src/lib.rs", "x");
    write_file(ws.path(), "node_modules/pkg/index.js", "x");

    let root = writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap();
    let state = make_state(&writ_dir, Some(root));
    workspace_index::rebuild_blocking(&state.workspace_index);

    let hits = search_workspace_files_inner(&state, "main");
    assert_eq!(hits[0].path, "src/main.rs");

    let status = workspace_index_status_inner(&state);
    assert_eq!(status.file_count, 2, "node_modules must be excluded");
    assert!(status.has_workspace);
    assert!(!status.truncated);
}

#[test]
fn content_search_streams_batches_and_final_outcome() {
    let writ_dir = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    write_file(ws.path(), "a.rs", "let needle = 1;\nneedle again");
    write_file(ws.path(), "b.rs", "no match here");
    write_file(ws.path(), "c.rs", "third needle");

    let root = writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap();
    let counter = Arc::new(AtomicU64::new(0));
    let batches: Arc<Mutex<Vec<SearchBatch>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = batches.clone();
    let emit: Arc<dyn Fn(SearchBatch) + Send + Sync> =
        Arc::new(move |b| sink.lock().unwrap().push(b));

    let outcome = run_content_search(
        root,
        counter,
        "needle".to_string(),
        GrepLimits::default(),
        emit,
    )
    .unwrap();

    let batches = batches.lock().unwrap();
    // Every batch is stamped with generation 1 (first search on this counter).
    assert!(batches.iter().all(|b| b.generation == 1));
    // Exactly one final batch, carrying the outcome and no hits.
    let finals: Vec<&SearchBatch> = batches.iter().filter(|b| b.outcome.is_some()).collect();
    assert_eq!(finals.len(), 1);
    assert!(finals[0].hits.is_empty());
    // Three matching lines across two files.
    let delivered: usize = batches
        .iter()
        .filter(|b| b.outcome.is_none())
        .map(|b| b.hits.len())
        .sum();
    assert_eq!(delivered, 3);
    assert_eq!(outcome.hit_count, 3);
    assert!(!outcome.cancelled);
    assert!(!outcome.truncated);
}

#[test]
fn content_search_second_call_bumps_generation() {
    let ws = TempDir::new().unwrap();
    write_file(ws.path(), "a.rs", "needle");
    let root = writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let seen_generations: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

    for _ in 0..2 {
        let sink = seen_generations.clone();
        let emit: Arc<dyn Fn(SearchBatch) + Send + Sync> =
            Arc::new(move |b| sink.lock().unwrap().push(b.generation));
        run_content_search(
            root.clone(),
            counter.clone(),
            "needle".to_string(),
            GrepLimits::default(),
            emit,
        )
        .unwrap();
    }
    let gens = seen_generations.lock().unwrap();
    assert!(gens.contains(&1));
    assert!(
        gens.contains(&2),
        "the second search must run at generation 2"
    );
}

#[test]
fn content_search_superseded_mid_flight_reports_cancelled() {
    let ws = TempDir::new().unwrap();
    // Enough matching files that the walk cannot finish before the first batch
    // is emitted and supersedes the search.
    for i in 0..800 {
        write_file(ws.path(), &format!("f{i}.txt"), "needle");
    }
    let root = writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_for_emit = counter.clone();
    let bumped = Arc::new(AtomicBool::new(false));
    let emit: Arc<dyn Fn(SearchBatch) + Send + Sync> = Arc::new(move |b| {
        // On the first hit batch, start a "newer" search by bumping the counter.
        if b.outcome.is_none() && !bumped.swap(true, Ordering::SeqCst) {
            counter_for_emit.fetch_add(1, Ordering::SeqCst);
        }
    });

    // A high result cap so cancellation, not the cap, is what stops the walk.
    let limits = GrepLimits {
        max_results: 1_000_000,
        ..GrepLimits::default()
    };
    let outcome = run_content_search(root, counter, "needle".to_string(), limits, emit).unwrap();

    assert!(
        outcome.cancelled,
        "a superseded search must report cancelled"
    );
    assert!(!outcome.truncated);
    assert!(
        outcome.files_scanned < 800,
        "cancellation must stop the walk early, scanned {}",
        outcome.files_scanned
    );
}
