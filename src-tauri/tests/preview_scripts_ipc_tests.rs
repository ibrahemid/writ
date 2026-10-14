//! The preview-scripts commands and the wiring ADR-043 depends on.
//!
//! The two commands are exercised through their Tauri-free inner functions
//! against the app's state, and a render through the render command's
//! Tauri-free core, served by the protocol handler, shows the opt-in reaching
//! the document's CSP. The last tests read `lib.rs`: a command that is not in
//! the invoke handler cannot be called, and a navigation guard that is not
//! installed refuses nothing, however well either behaves on its own.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::notes::materialise::DownloadOutcome;
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
use writ_tauri_lib::commands::preview::{
    preview_get_scripts_inner, preview_inline_image_download_inner, preview_inline_image_inner,
    preview_set_scripts_inner, preview_url_prefix, render_into_cache, CachedRender,
    InlineImageResult,
};
use writ_tauri_lib::preview::csp::build_document_csp;
use writ_tauri_lib::preview::handler::{resolve_cached, RenderCache};
use writ_tauri_lib::preview::renderers::HtmlRenderer;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

const LIB_RS: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "commands::preview::preview_url_prefix",
    "commands::preview::preview_get_scripts",
    "commands::preview::preview_set_scripts",
    "commands::preview::preview_inline_image",
    "commands::preview::preview_inline_image_download",
];

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
        buffer_writes: Default::default(),
    }
}

/// A state holding one open buffer named `id`, at `source_path` when given,
/// with the HTML renderer registered.
fn state_with(dir: &TempDir, id: &str, source_path: Option<&Path>) -> AppState {
    let state = make_state(dir);
    let now = Utc::now();
    state
        .store
        .lock()
        .expect("store")
        .insert(&BufferDocument {
            id: id.to_string(),
            title: "Downloaded".to_string(),
            filename: "downloaded.html".to_string(),
            status: BufferStatus::Active,
            language: None,
            source_path: source_path.map(|path| path.to_string_lossy().into_owned()),
            cursor_pos: 0,
            scroll_pos: 0,
            tab_order: 0,
            created_at: now,
            updated_at: now,
            closed_at: None,
            read_only: false,
            size_bytes: 0,
            line_ending: writ_core::notes::line_ending::LineEnding::Lf,
            encoding: None,
        })
        .expect("insert");
    state
        .preview_registry
        .write()
        .expect("registry")
        .register(Box::new(HtmlRenderer))
        .expect("register the HTML renderer");
    state
}

#[test]
fn a_file_starts_with_its_scripts_off() {
    let dir = TempDir::new().expect("temp dir");
    let state = state_with(&dir, "buf-1", None);
    assert_eq!(preview_get_scripts_inner(&state, "buf-1"), Ok(false));
}

#[test]
fn turning_a_files_scripts_on_and_off_is_read_back() {
    let dir = TempDir::new().expect("temp dir");
    let state = state_with(&dir, "buf-1", None);

    preview_set_scripts_inner(&state, "buf-1", true).expect("on");
    assert_eq!(preview_get_scripts_inner(&state, "buf-1"), Ok(true));

    preview_set_scripts_inner(&state, "buf-1", false).expect("off");
    assert_eq!(preview_get_scripts_inner(&state, "buf-1"), Ok(false));
}

#[test]
fn a_file_the_store_does_not_hold_is_refused_and_reads_off() {
    let dir = TempDir::new().expect("temp dir");
    let state = state_with(&dir, "buf-1", None);

    let refused = preview_set_scripts_inner(&state, "no-such-buffer", true);
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(
        preview_get_scripts_inner(&state, "no-such-buffer"),
        Ok(false)
    );
    // The refusal touched no other row.
    assert_eq!(preview_get_scripts_inner(&state, "buf-1"), Ok(false));
}

#[test]
fn an_opt_in_covers_the_bytes_writ_holds_and_lapses_when_it_holds_others() {
    let dir = TempDir::new().expect("temp dir");
    let file = dir.path().join("report.html");
    std::fs::write(&file, "<p>read</p>").expect("write the file");
    let state = state_with(&dir, "buf-1", Some(&file));
    state.record_disk_state_bytes("buf-1", &file, b"<p>read</p>");

    preview_set_scripts_inner(&state, "buf-1", true).expect("on");
    assert_eq!(preview_get_scripts_inner(&state, "buf-1"), Ok(true));

    // What the editor's read records when the file it reads back holds
    // another program's bytes.
    state.record_disk_state_bytes("buf-1", &file, b"<script>two()</script>");
    assert_eq!(preview_get_scripts_inner(&state, "buf-1"), Ok(false));
    assert!(
        !state
            .store
            .lock()
            .expect("store")
            .preview_scripts("buf-1")
            .expect("row"),
        "the lapse is recorded on the row"
    );
}

/// A downloaded HTML file whose own script would run if it were allowed to.
fn html_request() -> RenderRequest {
    RenderRequest {
        content_type: HtmlRenderer::content_type_id(),
        buffer_text: "<p>x</p><script>document.title = 'ran'</script>".to_string(),
        theme: Default::default(),
        zoom: 1.0,
        assets: None,
    }
}

/// The CSP the protocol handler serves a buffer's cached document under.
fn served_csp(cache: &RenderCache, buffer_id: &str) -> String {
    let resolved = resolve_cached(
        &format!("writ-preview://document/{buffer_id}?v=1"),
        cache,
        &|_: &Path| false,
    );
    resolved
        .headers
        .iter()
        .find(|(name, _)| *name == "Content-Security-Policy")
        .map(|(_, value)| value.clone())
        .expect("the document response carries a CSP")
}

/// Whether a render answered that it cached the document with its scripts on.
fn rendered_with_scripts(rendered: CachedRender) -> bool {
    match rendered {
        CachedRender::Rendered {
            scripts_enabled, ..
        } => scripts_enabled,
        other => panic!("expected a render, got {other:?}"),
    }
}

#[test]
fn a_render_is_served_under_the_csp_its_files_opt_in_calls_for() {
    let dir = TempDir::new().expect("temp dir");
    let state = state_with(&dir, "buf-1", None);
    let cache = &state.preview_render_cache;
    let render = || render_into_cache(&state, "buf-1", html_request());

    assert!(!rendered_with_scripts(render()));
    assert_eq!(served_csp(cache, "buf-1"), build_document_csp(false));

    preview_set_scripts_inner(&state, "buf-1", true).expect("on");
    assert!(rendered_with_scripts(render()));
    assert_eq!(served_csp(cache, "buf-1"), build_document_csp(true));

    preview_set_scripts_inner(&state, "buf-1", false).expect("off");
    assert_eq!(
        served_csp(cache, "buf-1"),
        build_document_csp(false),
        "turning the scripts off serves the cached render with them off before any render"
    );
    assert!(!rendered_with_scripts(render()));
    assert_eq!(served_csp(cache, "buf-1"), build_document_csp(false));
}

#[test]
fn a_render_after_a_panic_poisoned_the_store_is_served_with_scripts_off() {
    let dir = TempDir::new().expect("temp dir");
    let state = state_with(&dir, "buf-1", None);
    preview_set_scripts_inner(&state, "buf-1", true).expect("on");
    assert!(rendered_with_scripts(render_into_cache(
        &state,
        "buf-1",
        html_request()
    )));

    let poisoned = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _store = state.store.lock().expect("store");
                panic!("a holder of the store lock panicked");
            })
            .join()
    });
    assert!(poisoned.is_err() && state.store.is_poisoned());

    assert!(!rendered_with_scripts(render_into_cache(
        &state,
        "buf-1",
        html_request()
    )));
    assert_eq!(
        served_csp(&state.preview_render_cache, "buf-1"),
        build_document_csp(false)
    );
}

#[test]
fn a_render_of_a_file_the_store_does_not_hold_is_served_with_scripts_off() {
    let dir = TempDir::new().expect("temp dir");
    let state = state_with(&dir, "buf-1", None);
    preview_set_scripts_inner(&state, "buf-1", true).expect("on");

    let rendered = render_into_cache(&state, "no-such-buffer", html_request());

    assert!(!rendered_with_scripts(rendered));
    assert_eq!(
        served_csp(&state.preview_render_cache, "no-such-buffer"),
        build_document_csp(false)
    );
}

#[test]
fn a_content_type_with_no_renderer_caches_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);

    let rendered = render_into_cache(&state, "buf-1", html_request());

    assert!(matches!(rendered, CachedRender::NoRenderer));
    assert!(state.preview_render_cache.get("buf-1").is_none());
}

#[test]
fn every_command_is_in_the_invoke_handler() {
    for command in COMMANDS {
        assert!(
            LIB_RS.contains(command),
            "{command} is not in the invoke handler, so the editor cannot call it"
        );
    }
}

#[test]
fn the_navigation_guard_is_installed_on_the_app() {
    let source: String = LIB_RS.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        source.contains(".plugin(tauri::plugin::Builder::<tauri::Wry>::new(\"writ-navguard\")"),
        "the navigation guard plugin is not registered on the builder"
    );
    assert!(
        source.contains(
            ".on_navigation(|webview,url|{navigation::is_allowed_navigation(url.as_str(),\
             navigation::dev_server(webview.config()),)})"
        ),
        "the guard plugin does not route navigations through navigation::is_allowed_navigation \
         with the app's dev server"
    );
}

const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nrest";

fn never_dataless(_: &Path) -> bool {
    false
}

fn refusal_of(answer: InlineImageResult) -> String {
    match answer {
        InlineImageResult::Refused { reason } => reason,
        InlineImageResult::Ready { url, .. } => panic!("expected a refusal, got {url}"),
    }
}

fn ready_of(answer: InlineImageResult) -> (String, String) {
    match answer {
        InlineImageResult::Ready { url, mime, .. } => (url, mime),
        InlineImageResult::Refused { reason } => panic!("the image was refused: {reason}"),
    }
}

/// A file in a folder of its own beside an image, outside the notes folder.
fn image_beside_a_file(dir: &TempDir) -> std::path::PathBuf {
    let folder = dir.path().join("elsewhere");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("pic.png"), PNG_BYTES).expect("image");
    let file = folder.join("note.md");
    std::fs::write(&file, "![](pic.png)\n").expect("file");
    file
}

/// The webview names a buffer, and the folder holding that buffer's file
/// becomes a root images are read from. An id no open tab holds is refused,
/// or a folder Writ was never asked to show could become a root.
#[test]
fn an_inline_image_for_a_buffer_no_open_tab_holds_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    let state = state_with(&dir, "buf-1", Some(&file));

    let unknown =
        preview_inline_image_inner(&state, "buf-2", "pic.png", &never_dataless).expect("answer");
    assert_eq!(refusal_of(unknown), "outside_root");

    state
        .store
        .lock()
        .expect("store")
        .close("buf-1")
        .expect("close");
    let closed =
        preview_inline_image_inner(&state, "buf-1", "pic.png", &never_dataless).expect("answer");
    assert_eq!(refusal_of(closed), "outside_root");
}

#[test]
fn an_inline_image_resolves_against_the_open_tabs_folder() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    let state = state_with(&dir, "buf-1", Some(&file));

    let answer =
        preview_inline_image_inner(&state, "buf-1", "pic.png", &never_dataless).expect("answer");

    let (url, mime) = ready_of(answer);
    assert_eq!(mime, "image/png");
    assert!(
        url.starts_with(&format!(
            "{}document/_note-asset/buf-1/",
            preview_url_prefix()
        )),
        "{url}"
    );
    assert!(url.ends_with("/d/pic.png"), "{url}");
}

#[test]
fn an_inline_image_of_a_buffer_with_no_file_resolves_against_the_notes_folder() {
    let dir = TempDir::new().expect("temp dir");
    let state = state_with(&dir, "buf-1", None);
    std::fs::write(state.notes_root().join("pic.png"), PNG_BYTES).expect("image");

    let answer =
        preview_inline_image_inner(&state, "buf-1", "pic.png", &never_dataless).expect("answer");

    assert!(ready_of(answer).0.ends_with("/n/pic.png"));
}

#[test]
fn an_inline_image_outside_both_roots_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    std::fs::write(dir.path().join("outside.png"), PNG_BYTES).expect("image");
    let state = state_with(&dir, "buf-1", Some(&file));

    let answer = preview_inline_image_inner(&state, "buf-1", "../outside.png", &never_dataless)
        .expect("answer");

    assert_eq!(refusal_of(answer), "outside_root");
}

/// A Retina screenshot is answered with a URL, and the scheme serves the
/// whole file from it with no preview rendered. Refused as too large when the
/// editor carried base64 under a 2 MB cap.
#[test]
fn an_eight_megabyte_image_is_answered_with_a_url_the_scheme_serves() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    let mut shot = PNG_BYTES.to_vec();
    shot.resize(8 * 1024 * 1024, 0);
    std::fs::write(file.with_file_name("shot.png"), &shot).expect("image");
    let state = state_with(&dir, "buf-1", Some(&file));

    let answer =
        preview_inline_image_inner(&state, "buf-1", "shot.png", &never_dataless).expect("answer");
    let (url, _) = ready_of(answer);

    // The handler receives the native form on every platform: WebView2's
    // `http://writ-preview.` requests reach it rewritten.
    let route = url
        .strip_prefix(&format!("{}document/", preview_url_prefix()))
        .expect("a document-scope URL");
    let served = resolve_cached(
        &format!("writ-preview://document/{route}"),
        &state.preview_render_cache,
        &|_: &Path| false,
    );
    assert_eq!(served.status, 200);
    assert_eq!(served.body.len(), shot.len());
}

/// An evicted image is named from its stat. The file holds markup, so a read
/// would have answered `not_an_image`: `not_downloaded` means nothing read it.
#[test]
fn a_dataless_image_is_refused_as_not_downloaded_without_being_read() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    std::fs::write(file.with_file_name("cloud.png"), b"<!doctype html>").expect("file");
    let state = state_with(&dir, "buf-1", Some(&file));

    let answer =
        preview_inline_image_inner(&state, "buf-1", "cloud.png", &|_: &Path| true).expect("answer");

    assert_eq!(refusal_of(answer), "not_downloaded");
}

#[test]
fn downloading_an_evicted_image_answers_with_its_url_once_the_bytes_land() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    let state = state_with(&dir, "buf-1", Some(&file));
    let evicted = std::cell::Cell::new(true);
    let downloads = std::cell::Cell::new(0);
    let download = |_: &Path| {
        downloads.set(downloads.get() + 1);
        evicted.set(false);
        DownloadOutcome::Done
    };

    let answer = preview_inline_image_download_inner(
        &state,
        "buf-1",
        "pic.png",
        &|_: &Path| evicted.get(),
        &download,
    )
    .expect("answer");

    assert_eq!(downloads.get(), 1);
    assert_eq!(ready_of(answer).1, "image/png");
}

#[test]
fn a_download_that_ends_without_the_bytes_stays_not_downloaded() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    let state = state_with(&dir, "buf-1", Some(&file));

    let answer = preview_inline_image_download_inner(
        &state,
        "buf-1",
        "pic.png",
        &|_: &Path| true,
        &|_: &Path| DownloadOutcome::TimedOut,
    )
    .expect("answer");

    assert_eq!(refusal_of(answer), "not_downloaded");
}

#[test]
fn an_image_already_here_is_not_downloaded_again() {
    let dir = TempDir::new().expect("temp dir");
    let file = image_beside_a_file(&dir);
    let state = state_with(&dir, "buf-1", Some(&file));

    let answer = preview_inline_image_download_inner(
        &state,
        "buf-1",
        "pic.png",
        &never_dataless,
        &|_: &Path| panic!("a file on this machine was downloaded"),
    )
    .expect("answer");

    assert_eq!(ready_of(answer).1, "image/png");
}
