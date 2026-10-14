//! Shell budgets (`docs/perf-budgets.md`, ADR-047): how long a command holds
//! the thread that dispatched it, which in the app is the main thread.
//!
//! Commands are dispatched through Tauri's mock runtime the way the webview
//! dispatches them (`Webview::on_message`), so a command that runs inline is
//! measured as running inline. The always-on tests block each command's work
//! and require the dispatch to return regardless. The timed tests are skipped
//! unless `WRIT_PERF_GATE` is set; `scripts/perf-gate.sh` runs them in release,
//! one at a time.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::http::{HeaderMap, HeaderValue};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow};
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
use writ_tauri_lib::commands::buffer::NOTE_ID_HEADER;
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

const MB: usize = 1024 * 1024;

/// The most a listed command may hold the dispatching thread: one frame at
/// 60 Hz. Only the release build under the perf gate is held to it.
const SHELL_BUDGET_MS: u128 = 16;

/// How long a dispatch that should return at once is given before the test
/// calls it inline.
const DISPATCH_PATIENCE: Duration = Duration::from_secs(5);

fn perf_gate() -> bool {
    std::env::var("WRIT_PERF_GATE").is_ok()
}

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
    let mut registry = ContentRendererRegistry::new();
    writ_tauri_lib::preview::renderers::register_builtins(&mut registry, Arc::clone(&notes_index))
        .expect("renderers");

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
        preview_registry: Arc::new(RwLock::new(registry)),
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

/// An app on the mock runtime holding `state`, with the budgeted commands
/// registered as the app registers them, and its window.
fn mock_app(state: AppState) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            writ_tauri_lib::commands::file::open_file_confirmed,
            writ_tauri_lib::commands::buffer::read_buffer_content,
            writ_tauri_lib::commands::buffer::save_buffer_content,
            writ_tauri_lib::commands::buffer::save_buffer_changes,
            writ_tauri_lib::commands::preview::preview_render,
            writ_tauri_lib::commands::workspace::search_workspace_files,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("window");
    (app, window)
}

/// A command sent and not yet answered.
struct Dispatched {
    /// How long the dispatching thread was held.
    held: Receiver<Duration>,
    answer: Receiver<InvokeResponse>,
}

impl Dispatched {
    fn held(&self) -> Duration {
        self.held
            .recv_timeout(DISPATCH_PATIENCE)
            .expect("the dispatch held its thread while the command's work waited")
    }

    fn answer(&self) -> Result<InvokeResponseBody, Value> {
        match self
            .answer
            .recv_timeout(Duration::from_secs(600))
            .expect("the command answered")
        {
            InvokeResponse::Ok(body) => Ok(body),
            InvokeResponse::Err(error) => Err(error.0),
        }
    }
}

/// Sends `cmd` with `args` from a thread of its own, the way the webview
/// does: the time `on_message` holds that thread is what the main thread
/// would be held for.
fn dispatch(window: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Dispatched {
    send(window, cmd, InvokeBody::Json(args), HeaderMap::new())
}

/// Sends a save of note `id` the way the editor does: the text as the raw
/// body, the note in a header.
fn dispatch_save(window: &WebviewWindow<MockRuntime>, id: &str, text: &str) -> Dispatched {
    let mut headers = HeaderMap::new();
    headers.insert(NOTE_ID_HEADER, HeaderValue::from_str(id).expect("ascii id"));
    send(
        window,
        "save_buffer_content",
        InvokeBody::Raw(text.as_bytes().to_vec()),
        headers,
    )
}

fn send(
    window: &WebviewWindow<MockRuntime>,
    cmd: &str,
    body: InvokeBody,
    headers: HeaderMap,
) -> Dispatched {
    let (held_tx, held) = mpsc::channel();
    let (answer_tx, answer) = mpsc::channel();
    let webview = window.as_ref().clone();
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .expect("url"),
        body,
        headers,
        invoke_key: INVOKE_KEY.to_string(),
    };
    std::thread::spawn(move || {
        let start = Instant::now();
        webview.on_message(
            request,
            Box::new(move |_webview, _cmd, response, _callback, _error| {
                let _ = answer_tx.send(response);
            }),
        );
        let _ = held_tx.send(start.elapsed());
    });
    Dispatched { held, answer }
}

fn opened_id(answer: Result<InvokeResponseBody, Value>) -> String {
    let Ok(InvokeResponseBody::Json(json)) = answer else {
        panic!("the open failed: {answer:?}");
    };
    let value: Value = serde_json::from_str(&json).expect("json");
    value["doc"]["id"]
        .as_str()
        .expect("an opened row")
        .to_string()
}

fn write_log(path: &Path, bytes: usize) {
    let line = "2026-10-14T09:00:00Z INFO writ: a line of an ordinary log file\n";
    let mut text = String::with_capacity(bytes + line.len());
    while text.len() < bytes {
        text.push_str(line);
    }
    std::fs::write(path, text).expect("write fixture");
}

/// Authorizes `path` for one open, the way the picker does.
fn authorize(app: &tauri::App<MockRuntime>, path: &Path) -> String {
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    app.state::<AppState>()
        .authorized_paths
        .record_for_open(canonical.clone());
    canonical
}

/// Opens `path` through the store, outside the budget.
fn open(app: &tauri::App<MockRuntime>, path: &Path) -> String {
    let canonical = authorize(app, path);
    open_file_from_path(&app.state::<AppState>(), &canonical)
        .expect("open")
        .doc
        .expect("a row")
        .id
}

#[test]
fn open_file_confirmed_returns_its_thread_while_its_work_waits() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("a.log");
    write_log(&path, 4096);
    let canonical = authorize(&app, &path);

    let state = app.state::<AppState>();
    let store = state.store.lock().expect("store");
    let sent = dispatch(&window, "open_file_confirmed", json!({ "path": canonical }));
    sent.held();
    drop(store);
    assert!(!opened_id(sent.answer()).is_empty());
}

#[test]
fn read_buffer_content_returns_its_thread_while_its_work_waits() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("a.md");
    std::fs::write(&path, "text\n").unwrap();
    let id = open(&app, &path);

    let turn = app.state::<AppState>().buffer_writes.turn(&id);
    let sent = dispatch(&window, "read_buffer_content", json!({ "id": id }));
    sent.held();
    drop(turn);
    match sent.answer() {
        Ok(InvokeResponseBody::Raw(bytes)) => assert_eq!(bytes, b"text\n"),
        other => panic!("the read failed: {other:?}"),
    }
}

#[test]
fn save_buffer_content_returns_its_thread_while_its_work_waits() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("a.md");
    std::fs::write(&path, "text\n").unwrap();
    let id = open(&app, &path);

    let turn = app.state::<AppState>().buffer_writes.turn(&id);
    let sent = dispatch_save(&window, &id, "saved\n");
    sent.held();
    drop(turn);
    assert!(sent.answer().is_ok());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved\n");
}

#[test]
fn save_buffer_changes_returns_its_thread_while_its_work_waits() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("a.md");
    std::fs::write(&path, "text\n").unwrap();
    let id = open(&app, &path);
    let base = writ_core::hash::comparison_digest_hex(b"text\n");

    let turn = app.state::<AppState>().buffer_writes.turn(&id);
    let sent = dispatch(
        &window,
        "save_buffer_changes",
        json!({ "id": id, "base": base, "changes": [[4, "saved"], 1], "length": 6 }),
    );
    sent.held();
    drop(turn);
    assert!(sent.answer().is_ok());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved\n");
}

#[test]
fn preview_render_returns_its_thread_while_its_work_waits() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));

    let turn = app
        .state::<AppState>()
        .preview_render_cache
        .render_ticket("b1")
        .wait();
    let sent = dispatch(
        &window,
        "preview_render",
        json!({
            "bufferId": "b1",
            "contentType": "markdown",
            "text": "# Title\n",
            "theme": "light",
            "zoom": 1.0,
        }),
    );
    sent.held();
    drop(turn);
    assert!(sent.answer().is_ok());
    assert!(app
        .state::<AppState>()
        .preview_render_cache
        .get("b1")
        .is_some());
}

#[test]
fn search_workspace_files_returns_its_thread_while_its_work_waits() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));

    let state = app.state::<AppState>();
    let index = state.workspace_index.write().expect("index");
    let sent = dispatch(&window, "search_workspace_files", json!({ "query": "a" }));
    sent.held();
    drop(index);
    assert!(sent.answer().is_ok());
}

/// Saves of one note sent one after the other land in that order, even when
/// all of them are waiting on an earlier write of the note when they arrive.
#[test]
fn saves_to_one_note_land_in_the_order_they_were_sent() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("a.md");
    std::fs::write(&path, "start\n").unwrap();
    let id = open(&app, &path);
    let state = app.state::<AppState>();

    let earlier = state.buffer_writes.turn(&id);
    let mut sent = Vec::new();
    for (n, text) in ["one\n", "two\n", "three\n"].into_iter().enumerate() {
        sent.push(dispatch_save(&window, &id, text));
        let deadline = Instant::now() + DISPATCH_PATIENCE;
        while state.buffer_writes.outstanding(&id) < n as u64 + 2 {
            assert!(Instant::now() < deadline, "save {n} never took its place");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    drop(earlier);

    for save in &sent {
        assert!(save.answer().is_ok());
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "three\n");
    assert_eq!(state.buffer_writes.outstanding(&id), 0);
}

fn millis(duration: Duration) -> u128 {
    duration.as_millis()
}

#[test]
fn shell_budget_of_open_read_and_save_at_300_mb() {
    if !perf_gate() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("big.log");
    write_log(&path, 300 * MB);
    let canonical = authorize(&app, &path);

    let start = Instant::now();
    let sent = dispatch(&window, "open_file_confirmed", json!({ "path": canonical }));
    let open_held = sent.held();
    let id = opened_id(sent.answer());
    let open_work = start.elapsed();

    let start = Instant::now();
    let sent = dispatch(&window, "read_buffer_content", json!({ "id": id }));
    let read_held = sent.held();
    let Ok(InvokeResponseBody::Raw(bytes)) = sent.answer() else {
        panic!("the read failed");
    };
    let read_work = start.elapsed();

    let mut text = String::from_utf8(bytes).expect("utf-8");
    text.replace_range(0..1, "3");
    let start = Instant::now();
    let sent = dispatch_save(&window, &id, &text);
    let save_held = sent.held();
    let Ok(InvokeResponseBody::Json(digest)) = sent.answer() else {
        panic!("the save failed");
    };
    let save_work = start.elapsed();

    let base: String = serde_json::from_str(&digest).expect("a digest");
    let start = Instant::now();
    let sent = dispatch(
        &window,
        "save_buffer_changes",
        json!({ "id": id, "base": base, "changes": [[1, "4"], text.len() - 1], "length": text.len() }),
    );
    let changes_held = sent.held();
    sent.answer().expect("changes");
    let changes_work = start.elapsed();

    eprintln!(
        "shell budget at 300 MB (held / work): open_file_confirmed {} / {} ms, read_buffer_content {} / {} ms, save_buffer_content {} / {} ms, save_buffer_changes {} / {} ms",
        millis(open_held),
        millis(open_work),
        millis(read_held),
        millis(read_work),
        millis(save_held),
        millis(save_work),
        millis(changes_held),
        millis(changes_work)
    );
    for (command, held) in [
        ("open_file_confirmed", open_held),
        ("read_buffer_content", read_held),
        ("save_buffer_content", save_held),
        ("save_buffer_changes", changes_held),
    ] {
        assert!(
            millis(held) <= SHELL_BUDGET_MS,
            "{command} held its thread {} ms (budget {SHELL_BUDGET_MS} ms)",
            millis(held)
        );
    }
}

#[test]
fn shell_budget_of_preview_render_of_a_long_note() {
    if !perf_gate() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let state = app.state::<AppState>();
    let notes_root = state.notes_root();
    for n in 0..200 {
        std::fs::write(notes_root.join(format!("Note {n}.md")), "# A note\n").unwrap();
    }
    state
        .notes_index
        .reconcile(&notes_root, &|| false, &|_| false)
        .expect("reconcile");
    let path = notes_root.join("Long.md");
    let mut text = String::new();
    let mut links = 0;
    while text.len() < 5 * MB {
        text.push_str("## Section\n\nA paragraph that links to ");
        text.push_str(&format!("[[Note {}]]", links % 200));
        text.push_str(" and goes on with ordinary words for a while.\n\n");
        links += 1;
    }
    std::fs::write(&path, &text).unwrap();
    let id = open(&app, &path);

    let start = Instant::now();
    let sent = dispatch(
        &window,
        "preview_render",
        json!({
            "bufferId": id,
            "contentType": "markdown",
            "text": text,
            "theme": "light",
            "zoom": 1.0,
        }),
    );
    let held = sent.held();
    sent.answer().expect("render");
    let work = start.elapsed();
    eprintln!(
        "shell budget: preview_render of 5 MB with {links} wikilinks (held / work) {} / {} ms",
        millis(held),
        millis(work)
    );
    assert!(millis(held) <= SHELL_BUDGET_MS);
}

#[test]
fn shell_budget_of_search_workspace_files_over_200k_files() {
    if !perf_gate() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let root = dir.path().join("workspace");
    for folder in 0..400 {
        let sub = root.join(format!("module-{folder}"));
        std::fs::create_dir_all(&sub).unwrap();
        for file in 0..500 {
            std::fs::write(sub.join(format!("source_file_{file}.rs")), b"").unwrap();
        }
    }
    let state = app.state::<AppState>();
    state
        .workspace_index
        .write()
        .expect("index")
        .set_root(Some(root));
    writ_tauri_lib::workspace_index::rebuild_blocking(&state.workspace_index);
    let file_count = state
        .workspace_index
        .read()
        .expect("index")
        .status()
        .file_count;
    assert_eq!(file_count, 200_000);

    let mut slowest_held = Duration::ZERO;
    let mut slowest_work = Duration::ZERO;
    for query in ["s", "so", "sou", "sour", "source_f", "source_file_42"] {
        let start = Instant::now();
        let sent = dispatch(&window, "search_workspace_files", json!({ "query": query }));
        slowest_held = slowest_held.max(sent.held());
        sent.answer().expect("search");
        slowest_work = slowest_work.max(start.elapsed());
    }
    eprintln!(
        "shell budget: search_workspace_files over 200k files, slowest keystroke (held / work) {} / {} ms",
        millis(slowest_held),
        millis(slowest_work)
    );
    assert!(millis(slowest_held) <= SHELL_BUDGET_MS);
}
