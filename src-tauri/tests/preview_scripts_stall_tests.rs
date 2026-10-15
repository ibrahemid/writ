//! The preview-scripts checks against a file whose reads never return.
//!
//! A tab open on a network share whose server went away holds a file nothing
//! can read: a read blocks until the share answers. A FIFO with no writer is
//! the same file on any Unix machine, as in `heartbeat_stall_tests.rs`. The
//! render of an opted-in document and the status bar's read of its opt-in
//! compare digests Writ already holds (ADR-043), so neither reads the file,
//! and neither holds the store across a read that a save of another tab would
//! then wait behind.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::{ContentRendererRegistry, RenderRequest};
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
use writ_tauri_lib::commands::preview::{
    preview_get_scripts_inner, preview_set_scripts_inner, render_into_cache, CachedRender,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::preview::renderers::HtmlRenderer;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

/// Long enough for a render, a save and a read on an idle machine many times
/// over, and short enough that a stalled run fails rather than hangs the
/// suite.
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

/// Opens `path`, holding `content`, the way the frontend reaches a file.
fn open_at(state: &AppState, path: &Path, content: &str) -> String {
    std::fs::write(path, content).expect("write");
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id
}

/// Puts a FIFO nobody writes to where the file at `path` was.
fn stall(path: &Path) {
    std::fs::remove_file(path).expect("remove the file");
    let made = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo failed for {}", path.display());
}

/// Lets go of every read still waiting on `path`, so a run that stalled ends.
/// Each open of the write end releases the readers waiting then; it runs on a
/// thread of its own, since one with no reader waits for one.
fn release_readers(path: PathBuf) {
    std::thread::spawn(move || loop {
        drop(std::fs::OpenOptions::new().write(true).open(&path));
        std::thread::sleep(Duration::from_millis(20));
    });
}

/// Runs `call` on its own thread and answers whether it returned within the
/// deadline.
fn returns_on_its_own<T: Send + 'static>(
    state: &Arc<AppState>,
    call: impl FnOnce(&AppState) -> T + Send + 'static,
) -> Option<T> {
    let (done, finished) = mpsc::channel();
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        let _ = done.send(call(&state));
    });
    finished.recv_timeout(DEADLINE).ok()
}

#[test]
fn an_opted_in_tab_on_a_stalled_file_holds_up_no_save_and_no_status_read() {
    let dir = TempDir::new().expect("temp dir");
    let state = Arc::new(make_state(&dir));
    state
        .preview_registry
        .write()
        .expect("registry")
        .register(Box::new(HtmlRenderer))
        .expect("register the HTML renderer");
    let share = dir.path().join("share");
    std::fs::create_dir_all(&share).expect("share folder");
    let stalled = share.join("Stalled.html");
    let body = "<p>approved</p><script>one()</script>";
    let stalled_id = open_at(&state, &stalled, body);
    preview_set_scripts_inner(&state, &stalled_id, true).expect("opt the file in");
    let typing = open_at(
        &state,
        &state.notes_root().join("Typing.md"),
        "first line\n",
    );
    stall(&stalled);

    let render_id = stalled_id.clone();
    let rendered = returns_on_its_own(&state, move |state| {
        render_into_cache(
            state,
            &render_id,
            RenderRequest {
                content_type: HtmlRenderer::content_type_id(),
                buffer_text: body.to_string(),
                theme: Default::default(),
                zoom: 1.0,
                assets: None,
            },
        )
    });
    let typing_id = typing.clone();
    let saved = returns_on_its_own(&state, move |state| {
        save_buffer_content_inner(state, &typing_id, "typed meanwhile\n")
    });
    let field_id = stalled_id.clone();
    let field = returns_on_its_own(&state, move |state| {
        preview_get_scripts_inner(state, &field_id)
    });

    let stalled_somewhere = rendered.is_none() || saved.is_none() || field.is_none();
    if stalled_somewhere {
        release_readers(stalled.clone());
    }
    assert!(
        rendered.is_some(),
        "the render of the opted-in tab read its file and waited on it"
    );
    assert!(
        saved.is_some(),
        "a save of another tab waited behind the opted-in tab's file"
    );
    assert!(
        field.is_some(),
        "the status bar's read of the opt-in waited on the file"
    );

    assert!(matches!(
        rendered,
        Some(CachedRender::Rendered {
            scripts_enabled: true,
            ..
        })
    ));
    assert_eq!(field, Some(Ok(true)));
    assert!(saved.expect("saved").is_ok());
    assert_eq!(
        std::fs::read_to_string(state.notes_root().join("Typing.md")).expect("read"),
        "typed meanwhile\n"
    );
}
