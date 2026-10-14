//! No subscriber on the app's bus takes the buffer store (ADR-043 §1).
//!
//! The bus runs its subscribers on the emitting thread, and some emitters hold
//! the store while they emit, so a subscriber that took it would wait on its
//! own thread for good, with every later save and journal write behind it.
//! These tests hang the app's own subscribers on a state built here, hold the
//! store and emit one of every event; and they drive the two opens that
//! announce a change from under the store, a reopen of a tab whose file
//! another program changed and Today's note on a changed file, which also turn
//! the file's preview scripts off.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::Duration;

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::preview::{ContentRendererRegistry, RenderRequest};
use writ_core::update::UpdatePhase;
use writ_core::watcher::change_event::ExternalChange;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::bus_subscribers::{attach_bus_subscribers, BusHost};
use writ_tauri_lib::commands::activity::announce_waiting_inner;
use writ_tauri_lib::commands::buffer::read_buffer_content_inner;
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::notes::open_or_mint_dated_note;
use writ_tauri_lib::commands::preview::{
    preview_set_scripts_inner, render_into_cache, CachedRender,
};
use writ_tauri_lib::events::WritFrontendEvent;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::preview::renderers::HtmlRenderer;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

/// Long enough for an emit or an open on an idle machine many times over, and
/// short enough that one waiting on the store fails rather than hangs the
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

/// The application, as its subscribers reach it, with the frontend and the
/// menu bar left out.
#[derive(Clone)]
struct Headless {
    state: Arc<AppState>,
}

impl BusHost for Headless {
    fn app_state(&self) -> &AppState {
        &self.state
    }

    fn to_frontend(&self, _event: WritFrontendEvent) {}

    fn redraw_menu_bar(&self) {}

    fn announce_waiting_clients(&self) {
        let _ = announce_waiting_inner(&self.state);
    }
}

/// A state with every subscriber the app hangs on its bus, and the HTML
/// renderer registered.
fn running(dir: &TempDir) -> Arc<AppState> {
    let state = Arc::new(make_state(dir));
    attach_bus_subscribers(&Headless {
        state: Arc::clone(&state),
    });
    state
        .preview_registry
        .write()
        .expect("registry")
        .register(Box::new(HtmlRenderer))
        .expect("register the HTML renderer");
    state
}

/// Renders `text` for `id` through the preview's render path, answering whether
/// the document was cached with its scripts on.
fn render(state: &AppState, id: &str, text: &str) -> bool {
    match render_into_cache(
        state,
        id,
        RenderRequest {
            content_type: HtmlRenderer::content_type_id(),
            buffer_text: text.to_string(),
            theme: Default::default(),
            zoom: 1.0,
            assets: None,
        },
    ) {
        CachedRender::Rendered {
            scripts_enabled, ..
        } => scripts_enabled,
        other => panic!("expected a render, got {other:?}"),
    }
}

/// A tab the editor has read, its scripts turned on and its preview rendered
/// with them.
fn shown_with_scripts_on(state: &AppState, id: &str) {
    let text = read_buffer_content_inner(state, id).expect("the editor reads the file");
    preview_set_scripts_inner(state, id, true).expect("turn the scripts on");
    assert!(render(state, id, &String::from_utf8(text).expect("utf-8")));
}

/// `name` in the notes folder holding `body`, opened as a tab and shown with
/// its scripts on.
fn opted_in_note(state: &AppState, name: &str, body: &str) -> (String, String) {
    let path = state.notes_root().join(name);
    std::fs::write(&path, body).expect("seed the file");
    let canonical = canonicalize_for_authorization(&path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let id = open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id;
    shown_with_scripts_on(state, &id);
    (id, canonical)
}

fn row_opted_in(state: &AppState, id: &str) -> bool {
    state
        .store
        .lock()
        .expect("store")
        .preview_scripts(id)
        .expect("read the row")
}

fn cached_scripts_on(state: &AppState, id: &str) -> bool {
    state
        .preview_render_cache
        .get(id)
        .expect("a cached render")
        .scripts_enabled
}

/// Runs `call` on its own thread, as a command runs, and answers what it
/// returned, or `None` when it did not return within the deadline.
fn on_a_worker<T: Send + 'static>(
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

fn external(buffer_id: &str, path: &str, change: ExternalChange) -> WritEvent {
    WritEvent::BufferExternal {
        buffer_id: buffer_id.to_string(),
        path: path.to_string(),
        change,
        new_path: None,
        disk_hash: None,
    }
}

/// One of every event the bus carries, about a tab that is open with its
/// scripts on.
fn every_event(state: &AppState, id: &str, path: &str) -> Vec<WritEvent> {
    let root = state.notes_root().to_string_lossy().into_owned();
    let events = vec![
        WritEvent::BufferOpened {
            id: id.to_string(),
            title: "report.html".to_string(),
        },
        WritEvent::ConfigChanged {
            keys: vec!["editor.font_size".to_string()],
        },
        external(id, path, ExternalChange::Modified),
        external(id, path, ExternalChange::Removed),
        external(id, path, ExternalChange::Moved),
        WritEvent::WorkspaceChanged {
            path: path.to_string(),
            removed: false,
        },
        WritEvent::NotesChanged {
            path: path.to_string(),
            removed: false,
        },
        WritEvent::NotesChanged {
            path: path.to_string(),
            removed: true,
        },
        WritEvent::NotesSwept { root },
        WritEvent::NotesIndexed {
            paths: vec![path.to_string()],
        },
        WritEvent::InboxFileArrived {
            path: path.to_string(),
        },
        WritEvent::HotkeyToggle,
        WritEvent::MenuAction {
            action: "file.save".to_string(),
        },
        WritEvent::McpPendingChanged,
        WritEvent::FlushBeforeQuit,
        WritEvent::PluginEvent {
            plugin_id: "writ.test".to_string(),
            data: serde_json::json!({}),
        },
    ];
    for event in &events {
        listed(event);
    }
    events
}

/// Names every kind of event with no wildcard, so a kind added to `WritEvent`
/// does not compile here until [`every_event`] emits one too.
fn listed(event: &WritEvent) {
    match event {
        WritEvent::BufferOpened { .. }
        | WritEvent::ConfigChanged { .. }
        | WritEvent::BufferExternal { .. }
        | WritEvent::WorkspaceChanged { .. }
        | WritEvent::NotesChanged { .. }
        | WritEvent::NotesSwept { .. }
        | WritEvent::NotesIndexed { .. }
        | WritEvent::InboxFileArrived { .. }
        | WritEvent::HotkeyToggle
        | WritEvent::MenuAction { .. }
        | WritEvent::McpPendingChanged
        | WritEvent::FlushBeforeQuit
        | WritEvent::PluginEvent { .. } => {}
    }
}

#[test]
fn no_event_waits_on_the_buffer_store() {
    let dir = TempDir::new().expect("temp dir");
    let state = running(&dir);
    let (id, path) = opted_in_note(&state, "report.html", "<p>one</p><script>one()</script>");
    let events = every_event(&state, &id, &path);

    let held = state.store.lock().expect("store");
    for event in events {
        let named = format!("{event:?}");
        let bus = Arc::clone(&state.event_bus);
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            bus.emit(event);
            let _ = done.send(());
        });
        assert!(
            finished.recv_timeout(DEADLINE).is_ok(),
            "a subscriber waited on the buffer store while {named} was emitted"
        );
    }
    drop(held);
}

#[test]
fn the_app_subscribes_to_its_bus_in_one_place() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut subscribing = Vec::new();
    let mut folders = vec![src.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).expect("read the source folder") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && std::fs::read_to_string(&path)
                    .expect("read the source")
                    .contains(".subscribe(")
            {
                subscribing.push(path.strip_prefix(&src).expect("inside src").to_path_buf());
            }
        }
    }
    subscribing.sort();
    assert_eq!(
        subscribing,
        vec![
            PathBuf::from("bus_subscribers.rs"),
            PathBuf::from("events").join("bus_bridge.rs"),
        ],
        "a subscriber outside bus_subscribers.rs is one the store test does not hang"
    );
    let lib = std::fs::read_to_string(src.join("lib.rs")).expect("read lib.rs");
    assert_eq!(
        lib.matches("bus_subscribers::attach_bus_subscribers(")
            .count(),
        1
    );
    assert!(!lib.contains("attach_bridge"));
}

#[test]
fn reopening_a_tab_whose_file_another_program_changed_answers_and_turns_its_scripts_off() {
    let dir = TempDir::new().expect("temp dir");
    let state = running(&dir);
    let (id, path) = opted_in_note(&state, "a.html", "<p>one</p><script>one()</script>");
    std::fs::write(
        &path,
        "<p>two, from another program</p><script>two()</script>",
    )
    .expect("rewrite the file");

    let reopen_path = path.clone();
    let reopened = on_a_worker(&state, move |state| {
        open_file_from_path(state, &reopen_path).map(|opened| opened.doc.map(|doc| doc.id))
    })
    .expect("the reopen never answered: the store was held across the bus");

    assert_eq!(reopened.expect("the reopen"), Some(id.clone()));
    assert!(
        !row_opted_in(&state, &id),
        "the changed file kept its opt-in"
    );
    assert!(
        !cached_scripts_on(&state, &id),
        "the cached render would still be served with the file's scripts on"
    );
}

#[test]
fn todays_note_on_a_file_another_program_changed_answers_and_turns_its_scripts_off() {
    let dir = TempDir::new().expect("temp dir");
    let state = running(&dir);
    let now = chrono::Utc::now();
    let minted = open_or_mint_dated_note(&state, now).expect("mint today's note");
    let id = minted.opened.doc.expect("the note opened").id;
    shown_with_scripts_on(&state, &id);
    std::fs::write(&minted.path, "changed by a sync from another machine\n")
        .expect("rewrite the file");

    let today = on_a_worker(&state, move |state| {
        open_or_mint_dated_note(state, now).map(|today| today.path)
    })
    .expect("Today's note never answered: the store was held across the bus");

    assert_eq!(today.expect("today's note"), minted.path);
    assert!(!row_opted_in(&state, &id));
    assert!(!cached_scripts_on(&state, &id));
}
