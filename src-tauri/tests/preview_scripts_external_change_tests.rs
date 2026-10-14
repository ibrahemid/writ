//! A file another program changes on disk loses its preview-scripts opt-in, and
//! a save Writ makes keeps it (ADR-043).
//!
//! The app's open-file watcher runs over a real folder with the app's tracking
//! and ignore set, and turns the opt-in off through that tracking before it
//! reports the change; a receiver on the bus stands in for the frontend. A
//! change no watcher sees, made while the tab is closed or while Writ is not
//! running, is caught when the editor reads the file back: the digest it
//! records is not the one the opt-in covers. Those cases run through the app's
//! own close, History reopen, read and render.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::preview::{ContentRendererRegistry, RenderRequest};
use writ_core::update::UpdatePhase;
use writ_core::watcher::change_event::ExternalChange;
use writ_core::watcher::ignore::DEFAULT_IGNORE_TTL;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::{
    close_buffer_inner, read_buffer_content_inner, save_buffer_content_inner,
};
use writ_tauri_lib::commands::chat::announce_note_change;
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::history::restore_buffer_inner;
use writ_tauri_lib::commands::preview::{
    lapse_scripts_opt_in_now, preview_get_scripts_inner, preview_set_scripts_inner,
    render_into_cache,
};
use writ_tauri_lib::preview::handler::{RenderCache, RenderedDoc};
use writ_tauri_lib::preview::renderers::HtmlRenderer;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;
use writ_tauri_lib::watcher::moves::FileTracking;
use writ_tauri_lib::watcher::open_files::start_open_file_watcher;

/// Long enough for the 500 ms debounce plus the platform's own notification
/// latency, and for the ignore stamp to be nowhere near expiry.
const SETTLE: Duration = Duration::from_secs(3);

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
        first_run_finished: AtomicBool::new(false),
        menu_apps: Mutex::new(None),
        retitle_watch: Arc::new(writ_tauri_lib::first_run::RetitleWatch::new()),
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
    }
}

/// Every change the bus carries for a tab.
fn frontend_view(state: &AppState) -> mpsc::Receiver<WritEvent> {
    let (tx, rx) = mpsc::channel();
    state.event_bus.subscribe(move |event| {
        if let WritEvent::BufferExternal { .. } = event {
            let _ = tx.send(event.clone());
        }
    });
    rx
}

/// A state with the open-file watcher following its tabs through the app's
/// tracking, and a receiver for the changes the frontend would hear.
fn watching_state(dir: &TempDir) -> (Arc<AppState>, mpsc::Receiver<WritEvent>) {
    let state = Arc::new(make_state(dir));
    let rx = frontend_view(&state);
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

/// `name` in `dir` opened as a tab, its scripts turned on and a render of it
/// cached with them on.
fn opted_in(state: &AppState, dir: &Path, name: &str, body: &str) -> (String, PathBuf) {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("seed the file");
    let canonical = canonicalize_for_authorization(&path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let id = open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id;
    preview_set_scripts_inner(state, &id, true).expect("turn the scripts on");
    state.preview_render_cache.put(
        id.clone(),
        RenderedDoc {
            html: body.to_string(),
            assets: None,
            scripts_enabled: true,
        },
    );
    (id, PathBuf::from(canonical))
}

fn scripts_on(state: &AppState, id: &str) -> bool {
    preview_get_scripts_inner(state, id).expect("read the opt-in")
}

/// The opt-in as the row records it, before any check.
fn row_opted_in(state: &AppState, id: &str) -> bool {
    state
        .store
        .lock()
        .expect("store")
        .preview_scripts(id)
        .expect("read the row")
}

/// The read the editor makes of a tab's file when it shows it, which is what
/// records the bytes Writ holds for it.
fn editor_reads(state: &AppState, id: &str) -> Vec<u8> {
    read_buffer_content_inner(state, id).expect("the editor reads the file")
}

fn cached_scripts_on(state: &AppState, id: &str) -> bool {
    state
        .preview_render_cache
        .get(id)
        .expect("a cached render")
        .scripts_enabled
}

/// Whether a render of `text` for `id`, through the preview's render path, is
/// cached with the file's scripts on.
fn rendered_with_scripts(state: &AppState, id: &str, text: &str) -> bool {
    {
        let mut registry = state.preview_registry.write().expect("registry");
        if registry.get(&HtmlRenderer::content_type_id()).is_none() {
            registry
                .register(Box::new(HtmlRenderer))
                .expect("register the HTML renderer");
        }
    }
    render_into_cache(
        state,
        id,
        RenderRequest {
            content_type: HtmlRenderer::content_type_id(),
            buffer_text: text.to_string(),
            theme: Default::default(),
            zoom: 1.0,
            assets: None,
        },
    );
    cached_scripts_on(state, id)
}

/// The changes the bus carried for `id` within `SETTLE`.
fn changes_to(rx: &mpsc::Receiver<WritEvent>, id: &str) -> Vec<ExternalChange> {
    let deadline = Instant::now() + SETTLE;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(WritEvent::BufferExternal {
                buffer_id, change, ..
            }) if buffer_id == id => seen.push(change),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    seen
}

fn write_by_temp_and_rename(path: &Path, bytes: &[u8]) {
    let temp = path.with_extension("writ-test-tmp");
    std::fs::write(&temp, bytes).expect("write temp");
    std::fs::rename(&temp, path).expect("rename over target");
}

#[test]
fn a_file_another_program_rewrites_loses_its_scripts_opt_in() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let (state, rx) = watching_state(&dir);
    let (id, file) = opted_in(
        &state,
        downloads.path(),
        "report.html",
        "<p>read</p><script>one()</script>",
    );

    write_by_temp_and_rename(&file, b"<p>downloaded again</p><script>two()</script>");

    assert_eq!(changes_to(&rx, &id), vec![ExternalChange::Modified]);
    assert!(
        !scripts_on(&state, &id),
        "the replaced file kept the opt-in given to the one it replaced"
    );
    assert!(
        !cached_scripts_on(&state, &id),
        "the cached render would still be served with the file's scripts on"
    );
}

#[test]
fn a_save_writ_makes_keeps_the_scripts_opt_in() {
    assert!(
        DEFAULT_IGNORE_TTL > SETTLE,
        "the stamp has to outlive the wait, or this passes for the wrong reason"
    );
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let (state, rx) = watching_state(&dir);
    let (id, file) = opted_in(&state, downloads.path(), "report.html", "<p>read</p>");

    save_buffer_content_inner(&state, &id, "<p>edited in Writ</p>").expect("save");

    assert_eq!(
        std::fs::read_to_string(&file).expect("read the file"),
        "<p>edited in Writ</p>"
    );
    assert_eq!(
        changes_to(&rx, &id),
        Vec::<ExternalChange>::new(),
        "Writ's own save came back as somebody else's change"
    );
    assert!(
        scripts_on(&state, &id),
        "a save Writ made turned the scripts off"
    );
    assert!(cached_scripts_on(&state, &id));
}

#[test]
fn a_file_another_program_moves_keeps_its_scripts_opt_in() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let (state, rx) = watching_state(&dir);
    let (id, file) = opted_in(&state, downloads.path(), "report.html", "<p>read</p>");

    std::fs::rename(&file, file.with_file_name("renamed.html")).expect("move the file");

    assert_eq!(changes_to(&rx, &id), vec![ExternalChange::Moved]);
    assert!(
        scripts_on(&state, &id),
        "a move changes no bytes, and it turned the scripts off"
    );
    assert!(cached_scripts_on(&state, &id));
}

#[test]
fn lapsing_answers_whether_the_scripts_were_on() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let state = make_state(&dir);
    let (id, _file) = opted_in(&state, downloads.path(), "report.html", "<p>read</p>");

    assert!(lapse_scripts_opt_in_now(&state, &id));
    assert!(!cached_scripts_on(&state, &id));
    assert!(!lapse_scripts_opt_in_now(&state, &id));
    assert!(!lapse_scripts_opt_in_now(&state, "no-such-buffer"));
    assert!(!scripts_on(&state, &id));
}

#[test]
fn a_proposal_the_chat_applies_to_an_opted_in_tab_turns_its_scripts_off() {
    // The pane writes a file open in a tab through the editor's own save,
    // which carries the opt-in over its write; the announcement is what takes
    // it back, as for any write that is not the editor's.
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let state = make_state(&dir);
    let (id, file) = opted_in(&state, downloads.path(), "report.html", "<p>read</p>");
    let proposed = "<p>read</p><script>fetch('/leak')</script>";

    save_buffer_content_inner(&state, &id, proposed).expect("the apply's save");
    assert!(
        row_opted_in(&state, &id),
        "the editor's save path carries the opt-in over the write"
    );
    announce_note_change(&state, &id, &file, proposed.as_bytes());

    assert!(!row_opted_in(&state, &id));
    assert!(!scripts_on(&state, &id));
    assert!(!cached_scripts_on(&state, &id));
    assert!(!rendered_with_scripts(&state, &id, proposed));
}

#[test]
fn a_file_another_program_rewrites_while_its_tab_is_closed_loses_its_scripts_opt_in() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let state = make_state(&dir);
    let (id, file) = opted_in(
        &state,
        downloads.path(),
        "report.html",
        "<p>read</p><script>one()</script>",
    );
    close_buffer_inner(&state, &id).expect("close the tab");

    write_by_temp_and_rename(&file, b"<p>downloaded again</p><script>two()</script>");

    restore_buffer_inner(&state, &id).expect("reopen from History");
    assert_eq!(
        editor_reads(&state, &id),
        b"<p>downloaded again</p><script>two()</script>"
    );
    assert!(
        !scripts_on(&state, &id),
        "the reopened file kept the opt-in given to the one it replaced"
    );
    assert!(!row_opted_in(&state, &id), "the lapse is recorded");
    assert!(!rendered_with_scripts(
        &state,
        &id,
        "<p>downloaded again</p><script>two()</script>"
    ));
}

#[test]
fn a_file_saved_in_writ_keeps_its_scripts_opt_in_when_its_tab_is_closed_and_reopened() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let state = make_state(&dir);
    let (id, _file) = opted_in(&state, downloads.path(), "report.html", "<p>read</p>");

    save_buffer_content_inner(&state, &id, "<p>edited in Writ</p><script>three()</script>")
        .expect("save");
    close_buffer_inner(&state, &id).expect("close the tab");
    restore_buffer_inner(&state, &id).expect("reopen from History");
    editor_reads(&state, &id);

    assert!(scripts_on(&state, &id));
    assert!(rendered_with_scripts(
        &state,
        &id,
        "<p>edited in Writ</p><script>three()</script>"
    ));
}

#[test]
fn a_file_changed_while_writ_is_not_running_loses_its_scripts_opt_in() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let (id, file) = {
        let state = make_state(&dir);
        opted_in(
            &state,
            downloads.path(),
            "report.html",
            "<p>read</p><script>one()</script>",
        )
    };

    let relaunched = make_state(&dir);
    editor_reads(&relaunched, &id);
    assert!(
        scripts_on(&relaunched, &id),
        "a relaunch over a file nothing changed turned the scripts off"
    );
    drop(relaunched);

    write_by_temp_and_rename(&file, b"<p>downloaded again</p><script>two()</script>");

    let relaunched = make_state(&dir);
    assert!(
        !scripts_on(&relaunched, &id),
        "a relaunch holds none of the file's bytes until the editor reads them"
    );
    assert!(row_opted_in(&relaunched, &id));
    editor_reads(&relaunched, &id);
    assert!(!scripts_on(&relaunched, &id));
    assert!(!row_opted_in(&relaunched, &id), "the lapse is recorded");
    assert!(!rendered_with_scripts(
        &relaunched,
        &id,
        "<p>downloaded again</p><script>two()</script>"
    ));
}

#[test]
fn a_crlf_file_saved_and_read_back_keeps_its_scripts_opt_in() {
    // The opt-in's digest and the bytes Writ records are both the file's raw
    // bytes: the open's, the save's after the file's own line ending goes back
    // on, and the editor's read. A file that keeps CRLF breaks none of them.
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let state = make_state(&dir);
    let (id, file) = opted_in(
        &state,
        downloads.path(),
        "report.html",
        "<p>read</p>\r\n<script>one()</script>\r\n",
    );
    assert!(scripts_on(&state, &id));

    save_buffer_content_inner(&state, &id, "<p>edited</p>\n<script>two()</script>\n")
        .expect("save");
    assert_eq!(
        std::fs::read(&file).expect("read the file"),
        b"<p>edited</p>\r\n<script>two()</script>\r\n"
    );
    assert!(scripts_on(&state, &id));

    close_buffer_inner(&state, &id).expect("close the tab");
    restore_buffer_inner(&state, &id).expect("reopen from History");
    editor_reads(&state, &id);
    assert!(scripts_on(&state, &id));
    assert!(rendered_with_scripts(
        &state,
        &id,
        "<p>edited</p>\n<script>two()</script>\n"
    ));
}
