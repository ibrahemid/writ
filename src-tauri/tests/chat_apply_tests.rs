//! What an applied proposal does beyond the file: what it reports, what it
//! tells the tab holding the note, and what the next unclean relaunch makes of
//! it.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::hash::{sha256_bytes, sha256_hex};
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_core::watcher::change_event::{modification_is_news, ExternalChange};
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::chat::{announce_applied_note, apply_proposal_inner};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;
use writ_tauri_lib::watcher::moves::FileTracking;
use writ_tauri_lib::watcher::open_files::{start_open_file_watcher, NoOpenNotes};

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
/// A state with the open-file watcher running, and every event it raises.
fn watching_state(dir: &TempDir) -> (Arc<AppState>, mpsc::Receiver<WritEvent>) {
    let state = Arc::new(make_state(dir));
    let (tx, rx) = mpsc::channel();
    state.event_bus.subscribe(move |event| {
        let _ = tx.send(event.clone());
    });
    *state.file_tracking.lock().expect("tracking slot") = Some(FileTracking::of_state(&state));
    let open_files = start_open_file_watcher(
        state.event_bus.clone(),
        state.watcher_ignore.clone(),
        &state.notes_root(),
        state.file_tracking(),
    )
    .expect("start the open file watcher");
    *state.open_file_watcher.lock().expect("watcher slot") = Some(open_files);
    (state, rx)
}

/// A note on disk under the state's notes folder, and the digest it holds.
fn seed_note(state: &AppState, name: &str, body: &str) -> (std::path::PathBuf, String) {
    let file = state.notes_root().join(name);
    std::fs::write(&file, body).expect("seed a note");
    (file, sha256_hex(body.as_bytes()))
}

#[test]
fn applying_a_proposal_whose_text_the_note_already_holds_reports_no_change() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let (file, hash) = seed_note(&state, "Launch.md", "the same text\n");
    let before = std::fs::metadata(&file).expect("metadata").modified().ok();

    let outcome = apply_proposal_inner(
        &state.notes_root(),
        &NoOpenNotes,
        &state.writ_dir,
        "probe-host",
        &file.to_string_lossy(),
        "the same text\n",
        &hash,
        None,
        |_, _, _| unreachable!(),
    )
    .expect("a proposal the note already holds is not a refusal");

    assert!(
        !outcome.changed,
        "the card would say Applied for a write that never happened"
    );
    assert_eq!(outcome.bytes, "the same text\n".len() as u64);
    assert_eq!(
        std::fs::metadata(&file).expect("metadata").modified().ok(),
        before,
        "the file was rewritten"
    );
}

#[test]
fn applying_a_proposal_that_moves_bytes_reports_a_change() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let (file, hash) = seed_note(&state, "Launch.md", "before\n");

    let outcome = apply_proposal_inner(
        &state.notes_root(),
        &NoOpenNotes,
        &state.writ_dir,
        "probe-host",
        &file.to_string_lossy(),
        "after\n",
        &hash,
        None,
        |_, _, _| unreachable!(),
    )
    .expect("the proposal applies");

    assert!(outcome.changed);
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "after\n");
}

#[test]
fn an_identical_apply_records_no_bytes_written() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let (file, hash) = seed_note(&state, "Launch.md", "the same text\n");

    apply_proposal_inner(
        &state.notes_root(),
        &NoOpenNotes,
        &state.writ_dir,
        "probe-host",
        &file.to_string_lossy(),
        "the same text\n",
        &hash,
        None,
        |_, _, _| unreachable!(),
    )
    .expect("the apply returns");

    let records = writ_storage::activity_log::read_recent(&state.writ_dir, usize::MAX);
    let record = records
        .iter()
        .find(|record| record.action == "apply_proposal")
        .expect("a record of the apply");
    assert_eq!(
        record.bytes, None,
        "a write that moved nothing recorded a length"
    );
}

#[test]
fn an_applied_note_that_is_open_gets_one_external_change_for_its_tab() {
    let dir = TempDir::new().expect("temp dir");
    let (state, events) = watching_state(&dir);
    let (file, _) = seed_note(&state, "Launch.md", "before\n");
    state.follow_note_path("note-1", &file);
    while events.try_recv().is_ok() {}

    let applied = b"after\n";
    std::fs::write(&file, applied).expect("the apply writes");
    let told = announce_applied_note(&state, &file, applied);

    assert_eq!(told.as_deref(), Some("note-1"));
    let event = events.try_recv().expect("the tab was told");
    match event {
        WritEvent::BufferExternal {
            buffer_id, change, ..
        } => {
            assert_eq!(buffer_id, "note-1");
            assert_eq!(change, ExternalChange::Modified);
        }
        other => panic!("expected an external change, got {other:?}"),
    }

    // What the watcher will deliver for the same write, judged the way the
    // watcher judges it: with the bytes on record it is not news, so the tab
    // gets one event rather than a reload followed by a bar.
    let recorded = state.disk_state("note-1").expect("the bytes are on record");
    assert_eq!(recorded.hash, sha256_bytes(applied));
    assert!(
        !modification_is_news(Some(recorded.hash), Some(sha256_bytes(applied)), false),
        "the watcher's own delivery would reach the tab a second time"
    );
}

#[test]
fn an_applied_note_nobody_has_open_tells_no_tab() {
    let dir = TempDir::new().expect("temp dir");
    let (state, events) = watching_state(&dir);
    let (file, _) = seed_note(&state, "Launch.md", "before\n");
    while events.try_recv().is_ok() {}

    let applied = b"after\n";
    std::fs::write(&file, applied).expect("the apply writes");

    assert_eq!(announce_applied_note(&state, &file, applied), None);
    assert!(
        events.try_recv().is_err(),
        "a note nobody has open was announced to a tab"
    );
    assert!(state.disk_state("note-1").is_none());
}

/// A note the last session had open, with its row stamped where the launch
/// before this one left it.
fn open_row(state: &AppState, id: &str, file: &std::path::Path, content: &str) {
    let then = chrono::Utc::now() - chrono::Duration::seconds(30);
    let doc = writ_core::buffer::document::BufferDocument {
        id: id.to_string(),
        title: "Launch".to_string(),
        filename: format!("{id}.md"),
        status: writ_core::buffer::document::BufferStatus::Active,
        language: None,
        source_path: Some(file.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: then,
        updated_at: then,
        closed_at: None,
        read_only: false,
        size_bytes: content.len() as u64,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    };
    let store = state.store.lock().expect("store");
    store.open_from_path(&doc, content).expect("open the note");
}

#[test]
fn an_applied_edit_is_not_reverted_by_recovery() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let (file, hash) = seed_note(&state, "Launch.md", "before\n");
    open_row(&state, "note-1", &file, "before\n");

    // The 120 s heartbeat: every open note's file, read from disk, written as
    // an unclean snapshot.
    {
        let mut store = state.store.lock().expect("store");
        let contents = store.collect_buffer_contents().expect("collect");
        assert_eq!(contents.get("note-1").map(String::as_str), Some("before\n"));
        store
            .write_session_snapshot_if_changed(&contents)
            .expect("write the snapshot");
        assert_eq!(
            store.resolve_recovery().expect("resolve").len(),
            1,
            "the snapshot must be newer than the row, or this proves nothing"
        );
    }

    apply_proposal_inner(
        &state.notes_root(),
        &NoOpenNotes,
        &state.writ_dir,
        "probe-host",
        &file.to_string_lossy(),
        "after\n",
        &hash,
        None,
        |_, _, _| unreachable!(),
    )
    .expect("the proposal applies");

    // What the tab does with the change: it reads the file back, which is
    // also where the row learns what the file now holds.
    let read = writ_tauri_lib::commands::buffer::read_buffer_content_inner(&state, "note-1")
        .expect("the tab reloads");
    assert_eq!(String::from_utf8(read).expect("text"), "after\n");

    let restored = state
        .store
        .lock()
        .expect("store")
        .resolve_recovery()
        .expect("resolve");
    assert!(
        restored.is_empty(),
        "the last snapshot would be written back over the applied edit"
    );
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "after\n");
}
