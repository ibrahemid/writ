//! What `open_file` answers for a file it does not open straight away.
//!
//! A file over `THRESHOLD_LARGE_BYTES` is asked about before anything reads
//! it, and a file over `THRESHOLD_MAX_BYTES` is not opened at all. Both are
//! answers, not failures: the mode says which, `size_bytes` carries the size
//! the question names, and no row exists until the person says Open. A path is
//! never encoded into a string the frontend has to take apart, so a Windows
//! drive colon or a colon in a file name reaches the confirmed open intact.

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::buffer::document::BufferStatus;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::file_ops::{
    FileOpenMode, ERR_OPEN_TOO_LARGE, THRESHOLD_LARGE_BYTES, THRESHOLD_MAX_BYTES,
};
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
use writ_tauri_lib::commands::buffer::close_buffer_inner;
use writ_tauri_lib::commands::file::{
    open_file_confirmed_from_path, open_file_from_path, ERR_OPEN_NOT_AUTHORIZED,
};
use writ_tauri_lib::commands::history::{restore_buffer_confirmed_inner, restore_buffer_inner};
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

/// A text file of `size` bytes whose first 8 KiB are text and whose tail is a
/// hole. The binary sniff reads only the head, so the size alone decides the
/// tier, and nothing near `size` bytes is written to the test machine's disk.
fn sparse_text_file(path: &std::path::Path, size: u64) {
    let mut file = std::fs::File::create(path).expect("create");
    let line = b"2026-10-13 10:30:00 INFO a line of a log\n";
    let mut written = 0usize;
    while written < 8192 {
        file.write_all(line).expect("write head");
        written += line.len();
    }
    file.set_len(size).expect("extend");
}

fn active_row_for(state: &AppState, canonical: &str) -> bool {
    state
        .store
        .lock()
        .unwrap()
        .find_active_by_source_path(canonical)
        .unwrap()
        .is_some()
}

const SIXTY_MB: u64 = 60_000_000;

#[test]
fn a_file_over_fifty_mib_answers_the_confirm_as_data_and_reads_nothing() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("server.log");
    sparse_text_file(&file, SIXTY_MB);
    let canonical = canonicalize_for_authorization(&file).unwrap();

    let result = open_file_from_path(&state, &canonical)
        .expect("the confirm tier is an answer, not an error");

    assert_eq!(result.mode, FileOpenMode::LargeFileConfirm);
    assert_eq!(result.size_bytes, SIXTY_MB);
    assert!(result.doc.is_none(), "nothing is registered before Open");
    assert!(!active_row_for(&state, &canonical));
}

#[test]
fn the_confirm_tier_starts_one_byte_over_fifty_mib() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let at = state.notes_root().join("at.log");
    let over = state.notes_root().join("over.log");
    sparse_text_file(&at, THRESHOLD_LARGE_BYTES);
    sparse_text_file(&over, THRESHOLD_LARGE_BYTES + 1);

    let at = open_file_from_path(&state, &canonicalize_for_authorization(&at).unwrap())
        .expect("open at the limit");
    assert_eq!(at.mode, FileOpenMode::LargeFile);
    assert!(at.doc.is_some());

    let over = open_file_from_path(&state, &canonicalize_for_authorization(&over).unwrap())
        .expect("answer over the limit");
    assert_eq!(over.mode, FileOpenMode::LargeFileConfirm);
    assert!(over.doc.is_none());
}

// A colon is a legal file-name character on macOS and Linux. The old answer
// put the path inside "__CONFIRM_REQUIRED__:{path}:{size}", which the
// frontend split on ":", so this name confirmed at "…/10" with a size of 30 B.
#[cfg(unix)]
#[test]
fn a_colon_in_the_name_reaches_the_confirmed_open_intact() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("10:30.log");
    sparse_text_file(&file, SIXTY_MB);
    let canonical = canonicalize_for_authorization(&file).unwrap();

    let asked = open_file_from_path(&state, &canonical).expect("answer");
    assert_eq!(asked.mode, FileOpenMode::LargeFileConfirm);
    assert_eq!(asked.size_bytes, SIXTY_MB);

    let opened = open_file_confirmed_from_path(&state, &canonical).expect("open after Open");
    assert_eq!(opened.mode, FileOpenMode::LargeFile);
    let doc = opened.doc.expect("the confirmed open registers the file");
    assert_eq!(doc.source_path.as_deref(), Some(canonical.as_str()));
    assert_eq!(doc.size_bytes, SIXTY_MB);
}

#[test]
fn a_file_over_five_hundred_mib_is_refused_as_data_with_a_code() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("dump.log");
    // Refused is decided from the size before the sniff, so a hole is enough.
    std::fs::File::create(&file)
        .unwrap()
        .set_len(THRESHOLD_MAX_BYTES + 1)
        .unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();

    let result = open_file_from_path(&state, &canonical).expect("a refusal is an answer");

    let FileOpenMode::Refused { reason } = &result.mode else {
        panic!("expected a refusal, got {:?}", result.mode);
    };
    assert!(
        reason.starts_with(&format!("{ERR_OPEN_TOO_LARGE}: ")),
        "the reason leads with its code: {reason}"
    );
    assert_eq!(result.size_bytes, THRESHOLD_MAX_BYTES + 1);
    assert!(result.doc.is_none());
    assert!(!active_row_for(&state, &canonical));

    let confirmed = open_file_confirmed_from_path(&state, &canonical)
        .expect("the confirmed entry point refuses the same way");
    assert!(matches!(confirmed.mode, FileOpenMode::Refused { .. }));
    assert!(confirmed.doc.is_none());
}

// A path outside every root is opened on a one-shot grant. The question costs
// no grant: the answer hands it back, so the Open that follows is authorized.
#[test]
fn the_confirm_answer_keeps_the_grant_for_the_open_that_follows() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    let file = outside.join("big.log");
    sparse_text_file(&file, SIXTY_MB);
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    let asked = open_file_from_path(&state, &canonical).expect("answer");
    assert_eq!(asked.mode, FileOpenMode::LargeFileConfirm);
    assert_eq!(state.authorized_paths.pending_open_len(), 1);

    let opened = open_file_confirmed_from_path(&state, &canonical)
        .expect("the grant the question left authorizes the open");
    assert!(opened.doc.is_some());
}

// The confirm is a question asked before a read. A file of that size that is
// already open answers as what it is shown as: a large file.
#[test]
fn a_large_file_that_is_already_open_answers_as_a_large_file() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("open.log");
    sparse_text_file(&file, SIXTY_MB);
    let canonical = canonicalize_for_authorization(&file).unwrap();

    let first = open_file_confirmed_from_path(&state, &canonical).expect("first open");
    let first_id = first.doc.expect("row").id;

    let again = open_file_confirmed_from_path(&state, &canonical).expect("second open");
    assert_eq!(again.mode, FileOpenMode::LargeFile);
    assert_eq!(again.doc.expect("same row").id, first_id);
}

// The read that fails is named by a code the frontend words, never by the
// operating system's sentence.
#[cfg(unix)]
#[test]
fn a_file_without_read_permission_answers_with_the_read_code() {
    use std::os::unix::fs::PermissionsExt;
    use writ_tauri_lib::commands::buffer::ERR_READ_PERMISSION_DENIED;

    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("locked.txt");
    std::fs::write(&file, "secret").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&file).is_ok() {
        // Running as root reads through the mode; there is nothing to assert.
        return;
    }

    let error = open_file_from_path(&state, &canonical).expect_err("cannot be read");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(
        error.starts_with(&format!("{ERR_READ_PERMISSION_DENIED}: ")),
        "the failure leads with its code: {error}"
    );
}

// A path no folder and no gesture authorizes answers with a code the frontend
// words. A path that does not resolve answers with the same string, so the
// answer says nothing about what is on disk outside the folders.
#[test]
fn an_open_nothing_authorizes_answers_with_its_code_whether_or_not_the_file_is_there() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    let there = outside.join("there.txt");
    std::fs::write(&there, "text").unwrap();
    let missing = outside.join("missing.txt");

    let mut answers = Vec::new();
    for path in [&there, &missing] {
        let raw = path.to_string_lossy();
        let error = open_file_from_path(&state, &raw).expect_err("nothing authorizes it");
        assert!(
            error.starts_with(&format!("{ERR_OPEN_NOT_AUTHORIZED}: ")),
            "the failure leads with its code: {error}"
        );
        let confirmed =
            open_file_confirmed_from_path(&state, &raw).expect_err("nor the confirmed open");
        assert_eq!(confirmed, error);
        answers.push(error);
    }
    assert_eq!(answers[0], answers[1]);
}

// The question is asked before a read. A file of that size a tab already shows
// was read, so a plain open answers that tab as the large file it is shown as.
#[test]
fn a_confirm_tier_file_a_tab_shows_answers_that_tab() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("shown.log");
    sparse_text_file(&file, SIXTY_MB);
    let canonical = canonicalize_for_authorization(&file).unwrap();
    let first = open_file_confirmed_from_path(&state, &canonical).expect("open after Open");
    let first_id = first.doc.expect("row").id;

    let again = open_file_from_path(&state, &canonical).expect("open again");

    assert_eq!(again.mode, FileOpenMode::LargeFile);
    assert_eq!(again.size_bytes, SIXTY_MB);
    assert_eq!(again.doc.expect("the tab's row").id, first_id);
}

// The frontend finds an open tab by the path it was asked for. A path that
// spells the same file another way reaches Rust, which answers the tab too.
#[cfg(unix)]
#[test]
fn a_link_to_a_confirm_tier_file_a_tab_shows_answers_that_tab() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("target.log");
    sparse_text_file(&file, SIXTY_MB);
    let canonical = canonicalize_for_authorization(&file).unwrap();
    let first_id = open_file_confirmed_from_path(&state, &canonical)
        .expect("open after Open")
        .doc
        .expect("row")
        .id;
    let link = state.notes_root().join("link.log");
    std::os::unix::fs::symlink(&file, &link).unwrap();

    let again = open_file_from_path(&state, &link.to_string_lossy()).expect("open by the link");

    assert_eq!(again.mode, FileOpenMode::LargeFile);
    assert_eq!(again.doc.expect("the tab's row").id, first_id);
}

// Answering the tab reads the file again, which a file past the limit must not
// cost: it stays refused even with a tab open on it.
#[test]
fn a_file_in_a_tab_that_grew_past_the_limit_is_still_refused() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = state.notes_root().join("growing.log");
    sparse_text_file(&file, SIXTY_MB);
    let canonical = canonicalize_for_authorization(&file).unwrap();
    open_file_confirmed_from_path(&state, &canonical).expect("open after Open");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_len(THRESHOLD_MAX_BYTES + 1)
        .unwrap();

    let again = open_file_from_path(&state, &canonical).expect("an answer");

    assert!(matches!(again.mode, FileOpenMode::Refused { .. }));
    assert!(again.doc.is_none());
}

/// A short text file in the notes folder, opened and closed again.
fn closed_file(state: &AppState, name: &str) -> (std::path::PathBuf, String) {
    let file = state.notes_root().join(name);
    std::fs::write(&file, "a line of a log\n").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    let id = open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("row")
        .id;
    close_buffer_inner(state, &id).expect("close");
    (file, id)
}

fn grow(file: &std::path::Path, size: u64) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(file)
        .unwrap()
        .set_len(size)
        .unwrap();
}

fn status_of(state: &AppState, id: &str) -> BufferStatus {
    state.store.lock().unwrap().get(id).unwrap().status
}

// Reopening a closed file reads it. A file that grew past 50 MiB while it was
// closed is asked about first, and stays closed until the person says Open.
#[test]
fn a_closed_file_that_grew_past_fifty_mib_is_asked_about_before_it_reopens() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (file, id) = closed_file(&state, "grew.log");
    grow(&file, SIXTY_MB);

    let asked = restore_buffer_inner(&state, &id).expect("an answer");

    assert_eq!(asked.mode, FileOpenMode::LargeFileConfirm);
    assert_eq!(asked.size_bytes, SIXTY_MB);
    assert!(asked.doc.is_none());
    assert_eq!(status_of(&state, &id), BufferStatus::History);

    let reopened = restore_buffer_confirmed_inner(&state, &id).expect("reopen after Open");

    assert_eq!(reopened.mode, FileOpenMode::LargeFile);
    assert_eq!(reopened.size_bytes, SIXTY_MB);
    assert_eq!(reopened.doc.expect("the row").id, id);
    assert_eq!(status_of(&state, &id), BufferStatus::Active);
}

#[test]
fn a_closed_file_that_grew_past_the_limit_stays_closed() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (file, id) = closed_file(&state, "huge.log");
    grow(&file, THRESHOLD_MAX_BYTES + 1);

    for answer in [
        restore_buffer_inner(&state, &id).expect("an answer"),
        restore_buffer_confirmed_inner(&state, &id).expect("an answer after Open"),
    ] {
        let FileOpenMode::Refused { reason } = &answer.mode else {
            panic!("expected a refusal, got {:?}", answer.mode);
        };
        assert!(
            reason.starts_with(&format!("{ERR_OPEN_TOO_LARGE}: ")),
            "{reason}"
        );
        assert_eq!(answer.size_bytes, THRESHOLD_MAX_BYTES + 1);
        assert!(answer.doc.is_none());
    }
    assert_eq!(status_of(&state, &id), BufferStatus::History);
}

#[test]
fn a_closed_file_of_ordinary_size_reopens_at_once() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (_file, id) = closed_file(&state, "small.log");

    let reopened = restore_buffer_inner(&state, &id).expect("reopen");

    assert_eq!(reopened.mode, FileOpenMode::Normal);
    assert_eq!(reopened.doc.expect("the row").id, id);
    assert_eq!(status_of(&state, &id), BufferStatus::Active);
}

// Nothing is there to measure, so nothing is asked: the tab reopens and its
// read says the file is gone, as it did before.
#[test]
fn a_closed_file_that_is_gone_reopens_without_a_question() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let (file, id) = closed_file(&state, "gone.log");
    std::fs::remove_file(&file).unwrap();

    let reopened = restore_buffer_inner(&state, &id).expect("reopen");

    assert_eq!(reopened.doc.expect("the row").id, id);
    assert_eq!(status_of(&state, &id), BufferStatus::Active);
}
