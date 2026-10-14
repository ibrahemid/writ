//! The work ADR-047 moved off the main thread, as the functions behind the
//! commands do it: startup maintenance after the window shows, the note
//! search, closes and chat edits that wait for their notes' turns, and a
//! rename that reads its file with the store free.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

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
use writ_tauri_lib::commands::buffer::{close_buffers_inner, save_in_turn};
use writ_tauri_lib::commands::chat::apply_proposal_to_state;
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::history::search_buffers_inner;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

fn make_state(dir: &TempDir) -> AppState {
    let writ_dir = dir.path().join("data");
    let buffers_dir = writ_dir.join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    let notes_root = dir.path().join("Writ");
    std::fs::create_dir_all(&notes_root).expect("notes folder");
    let notes_root = writ_tauri_lib::security::canonicalize_root(&notes_root).expect("canonical");

    let db_path = writ_dir.join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let notes_index = Arc::new(NotesIndexStore::open(&db_path).expect("notes index db"));
    let mut store = BufferStore::new(conn, buffers_dir.clone());
    store.set_notes_root(notes_root.clone());
    store.set_notes_index(Arc::clone(&notes_index));
    let note_history = Arc::new(
        writ_storage::note_history::NoteHistoryStore::open(&writ_dir).expect("version store"),
    );

    AppState {
        store: Mutex::new(store),
        config_store: ConfigStore::new(writ_dir.join("config.toml")),
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        notes_root: RwLock::new(notes_root),
        first_run: false,
        first_run_finished: AtomicBool::new(false),
        menu_apps: Mutex::new(None),
        retitle_watch: Arc::new(writ_tauri_lib::first_run::RetitleWatch::new()),
        notes_root_fallback: RwLock::new(None),
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        notes_watcher: Mutex::new(None),
        open_file_watcher: Mutex::new(None),
        file_tracking: Mutex::new(None),
        notes_index,
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
        config_diagnostics: Mutex::new(Vec::new()),
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
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("a row")
        .id
}

#[test]
fn the_wait_for_the_window_ends_when_it_is_revealed() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(make_state(&dir));
    let revealer = Arc::clone(&state);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        revealer.window_revealed.store(true, Ordering::SeqCst);
    });
    assert!(state.wait_for_reveal(Duration::from_secs(10)));
}

#[test]
fn the_wait_for_a_window_nothing_shows_gives_up_at_its_limit() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    assert!(!state.wait_for_reveal(Duration::from_millis(60)));
}

#[test]
fn startup_maintenance_vacuums_a_database_that_is_mostly_free_pages() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let side = open_database(&state.writ_dir.join("writ.db")).expect("db");
    side.execute_batch(
        "CREATE TABLE bloat (body BLOB);
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2400)
         INSERT INTO bloat SELECT randomblob(4096) FROM n;
         DROP TABLE bloat;
         PRAGMA wal_checkpoint(TRUNCATE);",
    )
    .expect("bloat");
    drop(side);
    let before = state.store.lock().unwrap().database_stats().expect("stats");
    assert!(before.freelist_count > writ_core::maintenance::VACUUM_MIN_FREE_PAGES);

    state.run_startup_maintenance();
    let after = state.store.lock().unwrap().database_stats().expect("stats");
    assert_eq!(after.freelist_count, 0);
    assert!(after.page_count < before.page_count);
}

#[test]
fn the_note_search_names_the_tab_that_shows_a_hit() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let path = state.notes_root().join("Plan.md");
    std::fs::write(&path, "# Plan\n\nThe launch checklist.\n").unwrap();
    state
        .notes_index
        .reconcile(&state.notes_root(), &|| false, &|_| false)
        .expect("reconcile");
    let id = open(&state, &path);

    let found = search_buffers_inner(&state, "checkl").expect("search");
    assert_eq!(found.total, 1);
    assert_eq!(found.hits[0].buffer_id, id);
    assert_eq!(search_buffers_inner(&state, "").expect("empty").total, 0);
}

#[test]
fn a_close_waits_for_its_notes_turns_and_takes_each_once() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(make_state(&dir));
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    std::fs::write(&a, "a\n").unwrap();
    std::fs::write(&b, "b\n").unwrap();
    let a = open(&state, &a);
    let b = open(&state, &b);

    let held = state.buffer_writes.turn(&b);
    let (done, closed) = mpsc::channel();
    let closer = Arc::clone(&state);
    let ids = vec![b.clone(), a.clone(), b.clone()];
    std::thread::spawn(move || {
        done.send(close_buffers_inner(&closer, &ids)).unwrap();
    });
    assert!(
        closed.recv_timeout(Duration::from_millis(100)).is_err(),
        "the close ran while a write of one of its notes held the turn"
    );
    drop(held);
    closed
        .recv_timeout(Duration::from_secs(5))
        .expect("the close finished")
        .expect("closed");
    assert_eq!(state.buffer_writes.outstanding(&a), 0);
    assert_eq!(state.buffer_writes.outstanding(&b), 0);
}

#[test]
fn a_chat_edit_of_an_open_file_checks_it_after_a_save_still_landing() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("Plan.md");
    std::fs::write(&file, "X\n").unwrap();
    let file = std::path::PathBuf::from(canonicalize_for_authorization(&file).expect("canonical"));
    let id = open(&state, &file);
    let offered_against = writ_core::hash::sha256_hex(b"X\n");

    let landing = state.buffer_writes.turn(&id);
    std::thread::scope(|scope| {
        let applying = scope.spawn(|| {
            apply_proposal_to_state(
                &state,
                "test-host",
                &file.to_string_lossy(),
                "AI\n",
                &offered_against,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.buffer_writes.outstanding(&id) < 2 && !applying.is_finished() {
            assert!(Instant::now() < deadline, "the edit never queued");
            std::thread::sleep(Duration::from_millis(1));
        }
        save_in_turn(&state, &landing, "Y\n").expect("the save lands");
        drop(landing);
        let refused = applying
            .join()
            .expect("edit thread")
            .expect_err("the edit was checked against the file before the save landed");
        assert!(
            refused.contains("changed since this was proposed"),
            "{refused}"
        );
    });
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Y\n");
}

#[cfg(unix)]
#[test]
fn a_rename_reads_its_file_with_the_store_free() {
    use std::io::Write;
    use writ_tauri_lib::commands::notes::rename_note_inner;

    let dir = TempDir::new().unwrap();
    let state = Arc::new(make_state(&dir));
    let file = dir.path().join("big.log");
    std::fs::write(&file, "X\n").unwrap();
    let file = std::path::PathBuf::from(canonicalize_for_authorization(&file).expect("canonical"));
    let id = open(&state, &file);
    std::fs::remove_file(&file).unwrap();
    let made = std::process::Command::new("mkfifo")
        .arg(&file)
        .status()
        .expect("mkfifo");
    assert!(made.success(), "mkfifo failed");

    let (reading, read_started) = mpsc::channel();
    let (go_on, let_go) = mpsc::channel::<()>();
    let pipe = file.clone();
    std::thread::spawn(move || {
        // The guard's read, then the stamp's: each opens the pipe once.
        for read in 0..2 {
            let mut writer = std::fs::OpenOptions::new()
                .write(true)
                .open(&pipe)
                .expect("open the pipe");
            if read == 0 {
                reading.send(()).unwrap();
                let_go.recv().unwrap();
            }
            writer.write_all(b"X\n").expect("feed the pipe");
            drop(writer);
            // The reader sees end-of-file only while no writer holds the pipe.
            // Reopening at once can land between its reads and feed it twice.
            std::thread::sleep(Duration::from_millis(200));
        }
    });
    let (done, renamed) = mpsc::channel();
    let renamer = Arc::clone(&state);
    let renamed_id = id.clone();
    std::thread::spawn(move || {
        done.send(rename_note_inner(&renamer, &renamed_id, "renamed"))
            .unwrap();
    });

    read_started
        .recv_timeout(Duration::from_secs(10))
        .expect("the rename read its file");
    let store_was_free = state.store.try_lock().is_ok();
    go_on.send(()).unwrap();
    let renamed = renamed
        .recv_timeout(Duration::from_secs(10))
        .expect("the rename finished");
    assert!(
        store_was_free,
        "the rename held the store while it read the file"
    );
    let renamed = renamed.expect("renamed");
    assert_eq!(
        renamed.source_path.as_deref(),
        Some(
            file.with_file_name("renamed.log")
                .to_string_lossy()
                .as_ref()
        )
    );
}
