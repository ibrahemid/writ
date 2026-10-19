//! The editor's saves as they arrive over IPC: the whole text as a raw body,
//! the large tiers' change sets, and where a tab was left (U32, EP-13).
//!
//! Commands are dispatched through Tauri's mock runtime with
//! `Webview::on_message`, so the request reaches each command the way the
//! webview sends it: bytes and headers for a raw-body save, JSON otherwise.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::http::{HeaderMap, HeaderValue};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow};
use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::hash::comparison_digest_hex;
use writ_core::notes::line_ending::LineEnding;
use writ_core::preview::ContentRendererRegistry;
use writ_core::text_encoding::TextEncoding;
use writ_core::update::UpdatePhase;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::{
    note_disk_state_inner, read_buffer_content_inner, NoteDiskAnswer, ERR_CHANGES_INVALID,
    ERR_CHANGES_STALE, ERR_FILE_CHANGED_ON_DISK, ERR_SAVE_BODY, NOTE_ID_HEADER,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::{restore_unsaved_text, AppState};
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
        pending_lines: Mutex::new(Default::default()),
        cli_waits: Mutex::new(Default::default()),
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

fn mock_app(state: AppState) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            writ_tauri_lib::commands::buffer::save_buffer_content,
            writ_tauri_lib::commands::buffer::save_buffer_changes,
            writ_tauri_lib::commands::buffer::set_buffer_view_position,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("window");
    (app, window)
}

/// Sends `cmd` and waits for its answer.
fn call(
    window: &WebviewWindow<MockRuntime>,
    cmd: &str,
    body: InvokeBody,
    headers: HeaderMap,
) -> Result<InvokeResponseBody, Value> {
    let (answer_tx, answer) = mpsc::channel();
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
    window.as_ref().clone().on_message(
        request,
        Box::new(move |_webview, _cmd, response, _callback, _error| {
            let _ = answer_tx.send(response);
        }),
    );
    match answer
        .recv_timeout(Duration::from_secs(30))
        .expect("the command answered")
    {
        InvokeResponse::Ok(body) => Ok(body),
        InvokeResponse::Err(error) => Err(error.0),
    }
}

fn headers_for(id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(NOTE_ID_HEADER, HeaderValue::from_str(id).expect("ascii id"));
    headers
}

/// A save of note `id` the way the editor sends it.
fn save_raw(
    window: &WebviewWindow<MockRuntime>,
    id: &str,
    bytes: &[u8],
) -> Result<Option<String>, Value> {
    call(
        window,
        "save_buffer_content",
        InvokeBody::Raw(bytes.to_vec()),
        headers_for(id),
    )
    .map(digest_of)
}

fn save_changes(
    window: &WebviewWindow<MockRuntime>,
    id: &str,
    base: &str,
    changes: Value,
    length: usize,
) -> Result<Option<String>, Value> {
    call(
        window,
        "save_buffer_changes",
        InvokeBody::Json(json!({ "id": id, "base": base, "changes": changes, "length": length })),
        HeaderMap::new(),
    )
    .map(digest_of)
}

fn digest_of(body: InvokeResponseBody) -> Option<String> {
    match body {
        InvokeResponseBody::Json(json) => serde_json::from_str(&json).expect("a digest or null"),
        InvokeResponseBody::Raw(_) => panic!("a save answers with JSON"),
    }
}

fn refusal_code(error: &Value) -> &str {
    error
        .as_str()
        .and_then(|message| message.split(':').next())
        .unwrap_or_default()
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Opens `path` through the store, the way the picker does.
fn open(app: &tauri::App<MockRuntime>, path: &std::path::Path) -> String {
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    let state = app.state::<AppState>();
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical)
        .expect("open")
        .doc
        .expect("a row")
        .id
}

const NON_ASCII: &str = "café · 中文 · مرحبا · 😀👍🏽\nsecond line\n";

#[test]
fn a_raw_body_save_round_trips_non_ascii_text() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("note.md");
    std::fs::write(&path, "start\n").unwrap();
    let id = open(&app, &path);

    let digest = save_raw(&window, &id, NON_ASCII.as_bytes()).expect("save");

    assert_eq!(std::fs::read(&path).unwrap(), NON_ASCII.as_bytes());
    assert_eq!(digest, Some(comparison_digest_hex(NON_ASCII.as_bytes())));
    let state = app.state::<AppState>();
    assert_eq!(
        read_buffer_content_inner(&state, &id).expect("read"),
        NON_ASCII.as_bytes()
    );
    let size = state.store.lock().unwrap().get(&id).unwrap().size_bytes;
    assert_eq!(size, NON_ASCII.len() as u64);
}

#[test]
fn a_raw_body_save_is_encoded_and_line_ended_as_its_file_is() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("legacy.txt");
    std::fs::write(&path, b"old\r\n").unwrap();
    let id = open(&app, &path);
    {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store
            .set_encoding(&id, Some(TextEncoding::from_label("windows-1252").unwrap()))
            .unwrap();
        store.set_line_ending(&id, LineEnding::CrLf).unwrap();
    }

    save_raw(&window, &id, "café\nnaïve\n".as_bytes()).expect("save");

    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"caf\xe9\r\nna\xefve\r\n".to_vec()
    );
}

#[test]
fn a_raw_body_save_that_is_not_utf8_writes_nothing() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("note.md");
    std::fs::write(&path, "kept\n").unwrap();
    let id = open(&app, &path);

    let refused = save_raw(&window, &id, &[0x63, 0xff, 0xfe]).unwrap_err();

    assert_eq!(refusal_code(&refused), ERR_SAVE_BODY);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "kept\n");
}

#[test]
fn a_raw_body_save_that_names_no_note_is_refused() {
    let dir = TempDir::new().unwrap();
    let (_app, window) = mock_app(make_state(&dir));

    let refused = call(
        &window,
        "save_buffer_content",
        InvokeBody::Raw(b"text".to_vec()),
        HeaderMap::new(),
    )
    .unwrap_err();

    assert_eq!(refusal_code(&refused), ERR_SAVE_BODY);
}

#[test]
fn a_save_sent_through_post_message_reads_its_bytes_from_the_json_array() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("note.md");
    std::fs::write(&path, "start\n").unwrap();
    let id = open(&app, &path);
    let bytes: Vec<Value> = NON_ASCII.bytes().map(Value::from).collect();

    call(
        &window,
        "save_buffer_content",
        InvokeBody::Json(Value::Array(bytes)),
        headers_for(&id),
    )
    .expect("save");

    assert_eq!(std::fs::read(&path).unwrap(), NON_ASCII.as_bytes());
}

#[test]
fn a_change_set_save_applies_the_changes_to_the_text_the_last_save_wrote() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("big.log");
    std::fs::write(&path, "start\n").unwrap();
    let id = open(&app, &path);
    let base = save_raw(&window, &id, NON_ASCII.as_bytes())
        .expect("save")
        .expect("a digest");

    // "café" becomes "cafe", the emoji pair becomes one, and a line is added.
    let expected = "cafe · 中文 · مرحبا · 🎉\nsecond line\nthird\n";
    let changes = json!([3, [1, "e"], 16, [6, "🎉"], 13, [0, "third", ""]]);
    let digest = save_changes(&window, &id, &base, changes, utf16_len(expected)).expect("changes");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    assert_eq!(digest, Some(comparison_digest_hex(expected.as_bytes())));
}

#[test]
fn a_change_set_save_goes_through_the_note_encoding_and_line_ending() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("legacy.txt");
    std::fs::write(&path, b"caf\xe9\r\nend\r\n").unwrap();
    let id = open(&app, &path);
    {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store
            .set_encoding(&id, Some(TextEncoding::from_label("windows-1252").unwrap()))
            .unwrap();
        store.set_line_ending(&id, LineEnding::CrLf).unwrap();
    }
    let base = comparison_digest_hex("café\nend\n".as_bytes());

    save_changes(&window, &id, &base, json!([5, [0, "naïve", ""], 4]), 15).expect("changes");

    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"caf\xe9\r\nna\xefve\r\nend\r\n".to_vec()
    );
}

#[test]
fn a_change_set_save_over_a_file_that_changed_writes_nothing() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("note.md");
    std::fs::write(&path, "start\n").unwrap();
    let id = open(&app, &path);
    let base = save_raw(&window, &id, b"mine\n")
        .expect("save")
        .expect("a digest");
    std::fs::write(&path, "theirs\n").unwrap();

    let refused = save_changes(&window, &id, &base, json!([4, [0, "!"], 1]), 6).unwrap_err();

    assert_eq!(refusal_code(&refused), ERR_CHANGES_STALE);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");
}

#[test]
fn a_change_set_that_does_not_give_the_editor_length_writes_nothing() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("note.md");
    std::fs::write(&path, "start\n").unwrap();
    let id = open(&app, &path);
    let base = save_raw(&window, &id, b"mine\n")
        .expect("save")
        .expect("a digest");

    let wrong_length = save_changes(&window, &id, &base, json!([4, [0, "!"], 1]), 5).unwrap_err();
    let wrong_base = save_changes(&window, &id, &base, json!([9, [0, "!"]]), 10).unwrap_err();

    assert_eq!(refusal_code(&wrong_length), ERR_CHANGES_INVALID);
    assert_eq!(refusal_code(&wrong_base), ERR_CHANGES_INVALID);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine\n");
}

#[test]
fn a_change_set_save_for_a_note_with_no_file_asks_for_the_whole_text() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let state = app.state::<AppState>();
    let doc = writ_core::buffer::manager::BufferManager::new()
        .create_buffer(None)
        .expect("mint");
    state.store.lock().unwrap().insert(&doc).expect("insert");

    let refused = save_changes(
        &window,
        &doc.id,
        &comparison_digest_hex(b""),
        json!([[0, "new"]]),
        3,
    )
    .unwrap_err();

    assert_eq!(refusal_code(&refused), ERR_CHANGES_STALE);
}

/// The read's bytes as the editor holds them: decoded as the webview's
/// `TextDecoder` decodes them, which drops a leading U+FEFF, with every line
/// break as `\n`, as CodeMirror keeps them.
fn as_editor_holds(bytes: Vec<u8>) -> String {
    let text = String::from_utf8(bytes).expect("the read hands over UTF-8");
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    String::from_utf8(writ_core::hash::normalise_line_endings(text.as_bytes()).into_owned())
        .expect("UTF-8")
}

/// The digest the open's `note_disk_state` answers with.
fn digest_at_open(app: &tauri::App<MockRuntime>, id: &str) -> String {
    match note_disk_state_inner(&app.state::<AppState>(), id).expect("the open asks") {
        NoteDiskAnswer::Described { disk } => disk.hash,
        other => panic!("the open found {other:?}"),
    }
}

/// What a tab holds once its open has answered: the text its load read and
/// the digest the open answered with, which its first save sends as the base.
fn load_tab(app: &tauri::App<MockRuntime>, id: &str) -> (String, String) {
    let read = read_buffer_content_inner(&app.state::<AppState>(), id).expect("the load reads");
    (as_editor_holds(read), digest_at_open(app, id))
}

/// "naïve" typed as a line of its own after the first line of `text`, as the
/// change set CodeMirror makes of it, and the text the editor then holds.
fn type_second_line(text: &str) -> (Value, String) {
    let at = text.find('\n').expect("a first line") + 1;
    let typed = format!("{}naïve\n{}", &text[..at], &text[at..]);
    let changes = json!([
        utf16_len(&text[..at]),
        [0, "naïve", ""],
        utf16_len(&text[at..])
    ]);
    (changes, typed)
}

/// The first save after the open, as the large tiers send it: the changes
/// since the text the load read, against the digest the open answered with.
/// Answers with the text the editor holds after it.
fn save_first_change(
    app: &tauri::App<MockRuntime>,
    window: &WebviewWindow<MockRuntime>,
    id: &str,
) -> String {
    let (text, base) = load_tab(app, id);
    let (changes, typed) = type_second_line(&text);
    let digest = save_changes(window, id, &base, changes, utf16_len(&typed))
        .expect("the first save lands as its changes");
    assert_eq!(digest, Some(comparison_digest_hex(typed.as_bytes())));
    typed
}

/// The dated copies a refused save left in `folder`.
fn conflict_copies(folder: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut copies: Vec<_> = std::fs::read_dir(folder)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains("(conflict "))
        })
        .collect();
    copies.sort();
    copies
}

#[test]
fn the_first_save_after_the_open_keeps_a_crlf_file_crlf() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("windows.txt");
    std::fs::write(&path, b"one\r\ntwo\r\n").unwrap();
    let id = open(&app, &path);

    let typed = save_first_change(&app, &window, &id);

    assert_eq!(typed, "one\nnaïve\ntwo\n");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        "one\r\nnaïve\r\ntwo\r\n".as_bytes()
    );
}

#[test]
fn the_first_save_after_the_open_keeps_the_byte_order_mark() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("marked.txt");
    std::fs::write(&path, b"\xef\xbb\xbfone\ntwo\n").unwrap();
    let id = open(&app, &path);

    let typed = save_first_change(&app, &window, &id);

    assert_eq!(typed, "one\nnaïve\ntwo\n");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        "\u{feff}one\nnaïve\ntwo\n".as_bytes()
    );
}

#[test]
fn the_first_save_after_the_open_writes_windows_1252() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("legacy.txt");
    std::fs::write(&path, b"caf\xe9\r\nend\r\n").unwrap();
    let id = open(&app, &path);
    {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store
            .set_encoding(&id, Some(TextEncoding::from_label("windows-1252").unwrap()))
            .unwrap();
        store.set_line_ending(&id, LineEnding::CrLf).unwrap();
    }

    let typed = save_first_change(&app, &window, &id);

    assert_eq!(typed, "café\nnaïve\nend\n");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"caf\xe9\r\nna\xefve\r\nend\r\n".to_vec()
    );
}

#[test]
fn the_first_save_after_the_open_writes_utf16() {
    fn utf16le(text: &str) -> Vec<u8> {
        let mut out = b"\xff\xfe".to_vec();
        out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        out
    }
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("powershell.txt");
    std::fs::write(&path, utf16le("one\r\ntwo\r\n")).unwrap();
    let id = open(&app, &path);

    let typed = save_first_change(&app, &window, &id);

    assert_eq!(typed, "one\nnaïve\ntwo\n");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        utf16le("one\r\nnaïve\r\ntwo\r\n")
    );
}

#[test]
fn the_first_save_after_the_open_of_a_large_file_is_its_changes() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("big.log");
    let mut text = String::new();
    let mut line = 0;
    while text.len() as u64 <= writ_core::file_ops::THRESHOLD_NORMAL_BYTES {
        text.push_str(&format!("line {line} · café 😀\n"));
        line += 1;
    }
    std::fs::write(&path, &text).unwrap();
    let id = open(&app, &path);

    let typed = save_first_change(&app, &window, &id);

    assert_eq!(typed.len(), text.len() + "naïve\n".len());
    assert!(std::fs::read(&path).unwrap() == typed.as_bytes());
}

#[test]
fn the_first_save_after_a_relaunch_lands_on_the_text_the_journal_gave_back() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let state = app.state::<AppState>();
    let doc = writ_core::buffer::manager::BufferManager::new()
        .create_buffer(None)
        .expect("mint");
    state.store.lock().unwrap().insert(&doc).expect("insert");
    let kept = "kept · café\nsecond line\n";
    state
        .store
        .lock()
        .unwrap()
        .record_unsaved_text(&doc.id, kept, chrono::Utc::now())
        .expect("journaled");

    // The relaunch writes the text into the note's first file and seeds the
    // write guard with what it wrote.
    let restored = {
        let store = state.store.lock().unwrap();
        restore_unsaved_text(&store, &state.notes_root(), state.default_extension())
    };
    for (id, disk) in restored.disk_states {
        state.set_disk_state(&id, disk);
    }
    let path = state
        .store
        .lock()
        .unwrap()
        .get(&doc.id)
        .unwrap()
        .source_path
        .expect("the relaunch gave the note its file");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), kept);

    let typed = save_first_change(&app, &window, &doc.id);

    assert_eq!(typed, "kept · café\nnaïve\nsecond line\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), typed);
}

#[test]
fn a_file_changed_after_the_open_refuses_the_first_change_set() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let folder = dir.path().join("notes");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("shared.txt");
    std::fs::write(&path, b"mine one\r\nend\r\n").unwrap();
    let id = open(&app, &path);
    let (text, base) = load_tab(&app, &id);
    std::fs::write(&path, b"theirs\r\n").unwrap();

    let (changes, typed) = type_second_line(&text);
    let refused = save_changes(&window, &id, &base, changes, utf16_len(&typed)).unwrap_err();

    assert_eq!(refusal_code(&refused), ERR_CHANGES_STALE);
    assert_eq!(std::fs::read(&path).unwrap(), b"theirs\r\n".to_vec());
    assert!(conflict_copies(&folder).is_empty());

    // The whole text the editor sends next is refused as any save over a
    // change is, and set aside as the editor's text.
    let refused = save_raw(&window, &id, typed.as_bytes()).unwrap_err();
    assert_eq!(refusal_code(&refused), ERR_FILE_CHANGED_ON_DISK);
    assert_eq!(std::fs::read(&path).unwrap(), b"theirs\r\n".to_vec());
    let copies = conflict_copies(&folder);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(
        std::fs::read(&copies[0]).unwrap(),
        "mine one\r\nnaïve\r\nend\r\n".as_bytes()
    );
}

#[test]
fn a_file_changed_between_the_opens_read_and_its_digest_refuses_the_first_change_set() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let folder = dir.path().join("notes");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("shared.txt");
    std::fs::write(&path, b"draft one\r\nend\r\n").unwrap();
    let id = open(&app, &path);
    let read = read_buffer_content_inner(&app.state::<AppState>(), &id).expect("the load reads");
    let text = as_editor_holds(read);
    // Another program writes text of the same length before the open asks
    // for the file's digest, so the digest describes its text.
    std::fs::write(&path, b"DRAFT TWO\r\nEND\r\n").unwrap();
    let base = digest_at_open(&app, &id);

    let (changes, typed) = type_second_line(&text);
    let refused = save_changes(&window, &id, &base, changes, utf16_len(&typed)).unwrap_err();

    assert_eq!(refusal_code(&refused), ERR_CHANGES_STALE, "{refused}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"DRAFT TWO\r\nEND\r\n".to_vec()
    );
    assert!(
        conflict_copies(&folder).is_empty(),
        "the changes were applied to the other program's text"
    );

    let refused = save_raw(&window, &id, typed.as_bytes()).unwrap_err();
    assert_eq!(refusal_code(&refused), ERR_FILE_CHANGED_ON_DISK);
    let copies = conflict_copies(&folder);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(
        std::fs::read(&copies[0]).unwrap(),
        "draft one\r\nnaïve\r\nend\r\n".as_bytes()
    );
}

#[test]
fn a_change_set_for_a_file_writ_holds_no_record_of_asks_for_the_whole_text() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("note.md");
    std::fs::write(&path, "one\ntwo\n").unwrap();
    let id = open(&app, &path);
    let (text, base) = load_tab(&app, &id);
    app.state::<AppState>().forget_disk_state(&id);

    let (changes, typed) = type_second_line(&text);
    let refused = save_changes(&window, &id, &base, changes, utf16_len(&typed)).unwrap_err();

    assert_eq!(refusal_code(&refused), ERR_CHANGES_STALE);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
}

#[test]
fn the_view_position_reaches_the_row() {
    let dir = TempDir::new().unwrap();
    let (app, window) = mock_app(make_state(&dir));
    let path = dir.path().join("note.md");
    std::fs::write(&path, "text\n").unwrap();
    let id = open(&app, &path);

    call(
        &window,
        "set_buffer_view_position",
        InvokeBody::Json(json!({ "id": id, "cursorPos": 3, "scrollPos": 1 })),
        HeaderMap::new(),
    )
    .expect("position");

    let row = app
        .state::<AppState>()
        .store
        .lock()
        .unwrap()
        .get(&id)
        .unwrap();
    assert_eq!((row.cursor_pos, row.scroll_pos), (3, 1));
}
