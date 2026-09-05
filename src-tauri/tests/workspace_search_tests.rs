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
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_storage::workspace_grep::{GrepLimits, ScanObserver};
use writ_tauri_lib::commands::workspace::{
    run_content_search, search_workspace_files_inner, workspace_index_status_inner, SearchBatch,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
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

    let notes_root = writ_dir.join("Writ");
    std::fs::create_dir_all(&notes_root).unwrap();
    let notes_root = writ_tauri_lib::security::canonicalize_root(&notes_root).unwrap();

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
        notes_root: RwLock::new(notes_root),
        notes_root_fallback: RwLock::new(None),
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        notes_watcher: Mutex::new(None),
        open_file_watcher: Mutex::new(None),
        file_tracking: Mutex::new(None),
        notes_index: Arc::new(NotesIndexStore::open(&db_path).expect("notes index db")),
        notes_index_cancel: Arc::new(AtomicBool::new(false)),
        notes_reconcile: Arc::new(ReconcileGate::new()),
        quit: Arc::new(QuitState::new()),
        removal_holds: Default::default(),
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
        last_disk_hash: Mutex::new(std::collections::HashMap::new()),
        source_records: Mutex::new(std::collections::HashMap::new()),
        unsaved_on_exit: Mutex::new(std::collections::HashMap::new()),
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
        None,
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
            None,
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
    const FILES: usize = 800;
    for i in 0..FILES {
        write_file(ws.path(), &format!("f{i}.txt"), "needle");
    }
    let root = writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    // Start a "newer" search from inside the walk, on the first file it reaches:
    // the walk is then guaranteed to see the newer generation on its next
    // cancellation check, whatever the machine's load is doing to thread
    // scheduling.
    let counter_for_scan = counter.clone();
    let bumped = Arc::new(AtomicBool::new(false));
    let on_scanned: ScanObserver = Arc::new(move |_scanned| {
        if !bumped.swap(true, Ordering::SeqCst) {
            counter_for_scan.fetch_add(1, Ordering::SeqCst);
        }
    });

    let batches: Arc<Mutex<Vec<SearchBatch>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = batches.clone();
    let emit: Arc<dyn Fn(SearchBatch) + Send + Sync> =
        Arc::new(move |b| sink.lock().unwrap().push(b));

    // A high result cap so cancellation, not the cap, is what stops the walk.
    let limits = GrepLimits {
        max_results: 1_000_000,
        ..GrepLimits::default()
    };
    let outcome = run_content_search(
        root,
        counter,
        "needle".to_string(),
        limits,
        emit,
        Some(on_scanned),
    )
    .unwrap();

    assert!(
        outcome.cancelled,
        "a superseded search must report cancelled"
    );
    assert!(!outcome.truncated);
    assert!(
        outcome.files_scanned < FILES,
        "cancellation must stop the walk early, scanned {}",
        outcome.files_scanned
    );
    // The terminal batch still carries the outcome, stamped with the search's
    // own (now superseded) generation.
    let batches = batches.lock().unwrap();
    let finals: Vec<&SearchBatch> = batches.iter().filter(|b| b.outcome.is_some()).collect();
    assert_eq!(finals.len(), 1);
    assert_eq!(finals[0].generation, 1);
}

/// The notes watcher's subscriber, driven the way `lib.rs` wires it: one
/// changed path becomes one upsert, one removed path becomes one removal, and
/// a replayed event of either kind is a no-op rather than an error.
///
/// The event is classified directly rather than through a live watcher, so the
/// test does not wait on a 500 ms debounce or on the platform's file-system
/// notification latency.
#[test]
fn notes_watcher_upserts_one_path_and_removes_it_on_delete() {
    use std::time::{Duration, Instant};
    use writ_core::events::bus::WritEvent;
    use writ_tauri_lib::watcher::handler::classify_notes_event;

    let holder = TempDir::new().expect("tempdir");
    let state = make_state(&holder, None);
    let notes = state.notes_root();
    let note = notes.join("watched.md");
    std::fs::write(&note, "the kestrel hangs over the verge").expect("write note");

    let ignore = create_ignore_set();
    let ttl = Duration::from_secs(5);

    let created = classify_notes_event(&note, &notes, &ignore, ttl, Instant::now())
        .expect("a note another program wrote must be classified");
    let WritEvent::NotesChanged { path, removed } = created else {
        panic!("the notes watcher must emit NotesChanged");
    };
    assert!(!removed);
    assert!(state
        .notes_index
        .index_path(Path::new(&path))
        .expect("index_path"));

    let query = writ_core::search::to_prefix_match("kestrel").expect("query");
    let terms = writ_core::search::search_terms("kestrel");
    let hits = state
        .notes_index
        .search_hits(&query, &terms, 10)
        .expect("search");
    assert_eq!(hits.len(), 1, "the watched note is in the index");
    assert_eq!(
        hits[0].path.as_deref(),
        Some(writ_storage::notes_index::index_key(&note).as_str())
    );

    // Replaying the same event changes nothing.
    assert!(state
        .notes_index
        .index_path(Path::new(&path))
        .expect("replayed index_path"));
    assert_eq!(state.notes_index.snapshot().expect("snapshot").len(), 1);

    std::fs::remove_file(&note).expect("remove note");
    let deleted = classify_notes_event(&note, &notes, &ignore, ttl, Instant::now())
        .expect("a deleted note must be classified");
    let WritEvent::NotesChanged { path, removed } = deleted else {
        panic!("the notes watcher must emit NotesChanged");
    };
    assert!(removed, "a vanished file is reported as removed");
    state
        .notes_index
        .forget_path(Path::new(&path))
        .expect("forget_path");

    assert!(state
        .notes_index
        .search_hits(&query, &terms, 10)
        .expect("search")
        .is_empty());
    // A replayed delete is a no-op, which is what lets a rename arrive as one
    // delete plus one create in either order.
    state
        .notes_index
        .forget_path(Path::new(&path))
        .expect("replayed forget_path");
}

/// Writ's own save is stamped into the ignore set before it lands, so the
/// notes watcher never sees it: without the stamp the save would arrive back
/// as somebody else's edit.
#[test]
fn the_notes_watcher_suppresses_writs_own_write() {
    use std::time::{Duration, Instant};
    use writ_tauri_lib::watcher::handler::classify_notes_event;

    let holder = TempDir::new().expect("tempdir");
    let state = make_state(&holder, None);
    let notes = state.notes_root();
    let note = notes.join("saved.md");
    let body = b"written by writ";

    let ignore = create_ignore_set();
    let key = writ_core::watcher::ignore::source_key(
        // The spelling the watcher builds its key from: `canonicalize_root`
        // strips the Windows `\\?\` prefix, which the event path never carries.
        &writ_tauri_lib::security::canonicalize_root(&notes)
            .expect("canonical notes root")
            .join("saved.md"),
    );
    let now = Instant::now();
    ignore.lock().expect("ignore set").record(key, body, now);
    std::fs::write(&note, body).expect("write note");

    assert!(
        classify_notes_event(&note, &notes, &ignore, Duration::from_secs(5), now).is_none(),
        "a stamped write must not come back as an external change"
    );
}

/// The folders another client leaves behind never reach the index.
#[test]
fn the_notes_watcher_ignores_a_sync_clients_own_folders() {
    use std::time::{Duration, Instant};
    use writ_tauri_lib::watcher::handler::classify_notes_event;

    let holder = TempDir::new().expect("tempdir");
    let state = make_state(&holder, None);
    let notes = state.notes_root();
    let ignore = create_ignore_set();

    for folder in [".obsidian", ".trash", ".stfolder", ".stversions"] {
        let path = notes.join(folder).join("leftover.md");
        std::fs::create_dir_all(path.parent().unwrap()).expect("create folder");
        std::fs::write(&path, "not a note").expect("write");
        assert!(
            classify_notes_event(
                &path,
                &notes,
                &ignore,
                Duration::from_secs(5),
                Instant::now()
            )
            .is_none(),
            "{folder} must never reach the index"
        );
    }

    // The temp file every atomic write creates beside its target is dropped
    // too: turning one into a change event would reload the document registry
    // mid-edit.
    let tmp = notes.join(".tmpABC123");
    std::fs::write(&tmp, "half a write").expect("write");
    assert!(
        classify_notes_event(
            &tmp,
            &notes,
            &ignore,
            Duration::from_secs(5),
            Instant::now()
        )
        .is_none(),
        "an atomic write's temp file must never reach the index"
    );
}

/// The watcher is silent for the files nobody wrote on purpose and for nothing
/// else: temp files, half-finished downloads, the stub standing in for a file
/// that is not downloaded, and the folders a sync client keeps for itself. A
/// catch-up would otherwise fan out into an event per temp file.
///
/// A copy a sync service kept is never ignored, here or anywhere else: it holds
/// text somebody wrote, it is listed and flagged, and a change to it is
/// reported like a change to any other note.
#[test]
fn the_notes_watcher_is_silent_for_the_files_nobody_wrote_and_no_others() {
    use std::time::{Duration, Instant};
    use writ_core::events::bus::WritEvent;
    use writ_tauri_lib::watcher::handler::classify_notes_event;

    let holder = TempDir::new().expect("tempdir");
    let state = make_state(&holder, None);
    let notes = state.notes_root();
    let ignore = create_ignore_set();

    for name in [
        ".syncthing.note.md.tmp",
        "~syncthing~note.md.tmp",
        ".note.md.icloud",
        ".note.md.swp",
        "note.md~",
        "note.md.crdownload",
        ".obsidian.vimrc",
    ] {
        let path = notes.join(name);
        std::fs::write(&path, "not a note").expect("write");
        assert!(
            classify_notes_event(
                &path,
                &notes,
                &ignore,
                Duration::from_secs(5),
                Instant::now()
            )
            .is_none(),
            "{name} must never reach the index"
        );
    }

    // A folder a sync client keeps for itself churns with copies of notes; a
    // path through one is as invisible as the folder is.
    let cached = notes.join(".dropbox.cache").join("stale.md");
    std::fs::create_dir_all(cached.parent().unwrap()).expect("create folder");
    std::fs::write(&cached, "a copy of a note").expect("write");
    assert!(
        classify_notes_event(
            &cached,
            &notes,
            &ignore,
            Duration::from_secs(5),
            Instant::now()
        )
        .is_none(),
        "a sync client's own folder must never reach the index"
    );

    // The copy a sync client kept is somebody's text, so it is a change like
    // any other.
    let copy = notes.join("note.sync-conflict-20260822-120000-ABCD.md");
    std::fs::write(&copy, "both devices wrote").expect("write");
    assert!(
        matches!(
            classify_notes_event(
                &copy,
                &notes,
                &ignore,
                Duration::from_secs(5),
                Instant::now()
            ),
            Some(WritEvent::NotesChanged { removed: false, .. })
        ),
        "a copy that holds somebody's text is a change like any other"
    );
}
