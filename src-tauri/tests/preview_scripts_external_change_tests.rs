//! A file another program changes on disk loses its preview-scripts opt-in, and
//! a save Writ makes keeps it (ADR-043).
//!
//! The app's open-file watcher runs over a real folder with the app's tracking
//! and ignore set. The reset is subscribed to the bus ahead of a receiver that
//! stands in for the frontend, the order `lib.rs` subscribes it in ahead of the
//! bridge, which a test reads. A change no watcher sees, made while the tab is
//! closed or while Writ is not running, is caught by the digest recorded with
//! the opt-in, through the app's own close, History reopen and render.

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
use writ_tauri_lib::commands::buffer::{close_buffer_inner, save_buffer_content_inner};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::history::restore_buffer_inner;
use writ_tauri_lib::commands::preview::{
    forget_scripts_opt_in, preview_get_scripts_inner, preview_set_scripts_inner, render_into_cache,
    subscribe_scripts_reset,
};
use writ_tauri_lib::preview::handler::{RenderCache, RenderedDoc};
use writ_tauri_lib::preview::renderers::HtmlRenderer;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;
use writ_tauri_lib::watcher::moves::FileTracking;
use writ_tauri_lib::watcher::open_files::start_open_file_watcher;

const LIB_RS: &str = include_str!("../src/lib.rs");

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

/// The reset on `state`'s bus, reaching the state through a weak reference so
/// the bus it is stored in does not keep the state alive.
fn subscribe_reset(state: &Arc<AppState>) {
    let reset = Arc::downgrade(state);
    subscribe_scripts_reset(&state.event_bus, move |buffer_id| {
        if let Some(state) = reset.upgrade() {
            forget_scripts_opt_in(&state.store, &state.preview_render_cache, buffer_id);
        }
    });
}

/// Every change the bus carries after the reset has seen it.
fn frontend_view(state: &AppState) -> mpsc::Receiver<WritEvent> {
    let (tx, rx) = mpsc::channel();
    state.event_bus.subscribe(move |event| {
        if let WritEvent::BufferExternal { .. } = event {
            let _ = tx.send(event.clone());
        }
    });
    rx
}

/// A state with the reset subscribed, the open-file watcher following its tabs,
/// and a receiver for the changes the frontend would hear.
fn watching_state(dir: &TempDir) -> (Arc<AppState>, mpsc::Receiver<WritEvent>) {
    let state = Arc::new(make_state(dir));
    subscribe_reset(&state);
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
    {
        let store = state.store.lock().expect("store");
        preview_set_scripts_inner(&store, &id, true).expect("turn the scripts on");
    }
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
    let store = state.store.lock().expect("store");
    preview_get_scripts_inner(&store, id).expect("read the opt-in")
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
        &state.store,
        &state.preview_registry,
        &state.preview_render_cache,
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

fn external(buffer_id: &str, change: ExternalChange) -> WritEvent {
    WritEvent::BufferExternal {
        buffer_id: buffer_id.to_string(),
        path: "/downloads/report.html".to_string(),
        change,
        new_path: None,
        disk_hash: None,
    }
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
fn only_a_modification_turns_the_scripts_off() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let state = Arc::new(make_state(&dir));
    subscribe_reset(&state);
    let (id, _file) = opted_in(&state, downloads.path(), "report.html", "<p>read</p>");

    state.event_bus.emit(external(&id, ExternalChange::Moved));
    state.event_bus.emit(external(&id, ExternalChange::Removed));
    assert!(
        scripts_on(&state, &id),
        "a move or a removal turned the scripts off"
    );
    assert!(cached_scripts_on(&state, &id));

    state
        .event_bus
        .emit(external(&id, ExternalChange::Modified));
    assert!(!scripts_on(&state, &id));
    assert!(!cached_scripts_on(&state, &id));
}

#[test]
fn forgetting_answers_whether_the_scripts_were_on() {
    let dir = TempDir::new().expect("temp dir");
    let downloads = TempDir::new().expect("a folder outside the notes folder");
    let state = make_state(&dir);
    let (id, _file) = opted_in(&state, downloads.path(), "report.html", "<p>read</p>");

    assert!(forget_scripts_opt_in(
        &state.store,
        &state.preview_render_cache,
        &id
    ));
    assert!(!forget_scripts_opt_in(
        &state.store,
        &state.preview_render_cache,
        &id
    ));
    assert!(!forget_scripts_opt_in(
        &state.store,
        &state.preview_render_cache,
        "no-such-buffer"
    ));
    assert!(!scripts_on(&state, &id));
}

#[test]
fn the_app_subscribes_the_reset_ahead_of_the_frontend_bridge() {
    let reset = LIB_RS
        .find("commands::preview::subscribe_scripts_reset(")
        .expect("lib.rs subscribes the reset");
    let bridge = LIB_RS
        .find("bus_bridge::attach_bridge_behind_notes_index(")
        .expect("lib.rs attaches the bridge");
    assert!(
        reset < bridge,
        "the frontend would hear of the change before the opt-in is off"
    );
    assert!(LIB_RS.contains("commands::preview::forget_scripts_opt_in("));
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
    assert!(
        !scripts_on(&state, &id),
        "the reopened file kept the opt-in given to the one it replaced"
    );
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

    write_by_temp_and_rename(&file, b"<p>downloaded again</p><script>two()</script>");

    let relaunched = make_state(&dir);
    assert!(!scripts_on(&relaunched, &id));
    assert!(!rendered_with_scripts(
        &relaunched,
        &id,
        "<p>downloaded again</p><script>two()</script>"
    ));
}
