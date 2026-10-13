//! The preview-scripts commands and the wiring ADR-043 depends on.
//!
//! The two commands are exercised through their Tauri-free inner functions
//! against a real buffer store, and a render through the render command's
//! Tauri-free core, served by the protocol handler, shows the opt-in reaching
//! the document's CSP. The last tests read `lib.rs`: a command that is not in
//! the invoke handler cannot be called, and a navigation guard that is not
//! installed refuses nothing, however well either behaves on its own.

use std::path::Path;
use std::sync::{Mutex, RwLock};

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::preview::{ContentRendererRegistry, RenderRequest};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_tauri_lib::commands::preview::{
    preview_get_scripts_inner, preview_set_scripts_inner, render_into_cache, CachedRender,
};
use writ_tauri_lib::preview::csp::build_document_csp;
use writ_tauri_lib::preview::handler::{resolve_cached, RenderCache};
use writ_tauri_lib::preview::renderers::HtmlRenderer;

const LIB_RS: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "commands::preview::preview_url_prefix",
    "commands::preview::preview_get_scripts",
    "commands::preview::preview_set_scripts",
];

/// A store holding one open, unsaved buffer.
fn store_with(id: &str) -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("writ.db")).expect("open database");
    run_migrations(&conn).expect("migrations");
    let buffers = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers).expect("buffers dir");
    let store = BufferStore::new(conn, buffers);
    let now = Utc::now();
    store
        .insert(&BufferDocument {
            id: id.to_string(),
            title: "Downloaded".to_string(),
            filename: "downloaded.html".to_string(),
            status: BufferStatus::Active,
            language: None,
            source_path: None,
            cursor_pos: 0,
            scroll_pos: 0,
            tab_order: 0,
            created_at: now,
            updated_at: now,
            closed_at: None,
            read_only: false,
            size_bytes: 0,
            line_ending: writ_core::notes::line_ending::LineEnding::Lf,
        })
        .expect("insert");
    (dir, store)
}

#[test]
fn a_file_starts_with_its_scripts_off() {
    let (_dir, store) = store_with("buf-1");
    assert_eq!(preview_get_scripts_inner(&store, "buf-1"), Ok(false));
}

#[test]
fn turning_a_files_scripts_on_and_off_is_read_back() {
    let (_dir, store) = store_with("buf-1");

    preview_set_scripts_inner(&store, "buf-1", true).expect("on");
    assert_eq!(preview_get_scripts_inner(&store, "buf-1"), Ok(true));

    preview_set_scripts_inner(&store, "buf-1", false).expect("off");
    assert_eq!(preview_get_scripts_inner(&store, "buf-1"), Ok(false));
}

#[test]
fn a_file_the_store_does_not_hold_is_refused_and_reads_off() {
    let (_dir, store) = store_with("buf-1");

    let refused = preview_set_scripts_inner(&store, "no-such-buffer", true);
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(
        preview_get_scripts_inner(&store, "no-such-buffer"),
        Ok(false)
    );
    // The refusal touched no other row.
    assert_eq!(preview_get_scripts_inner(&store, "buf-1"), Ok(false));
}

/// A registry holding the HTML renderer alone.
fn html_registry() -> RwLock<ContentRendererRegistry> {
    let mut registry = ContentRendererRegistry::new();
    registry
        .register(Box::new(HtmlRenderer))
        .expect("register the HTML renderer");
    RwLock::new(registry)
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

#[test]
fn a_render_is_served_under_the_csp_its_files_opt_in_calls_for() {
    let (_dir, store) = store_with("buf-1");
    let store = Mutex::new(store);
    let registry = html_registry();
    let cache = RenderCache::new();
    let render = || render_into_cache(&store, &registry, &cache, "buf-1", html_request());

    assert!(matches!(render(), CachedRender::Rendered { .. }));
    assert_eq!(served_csp(&cache, "buf-1"), build_document_csp(false));

    preview_set_scripts_inner(&store.lock().expect("store"), "buf-1", true).expect("on");
    assert!(matches!(render(), CachedRender::Rendered { .. }));
    assert_eq!(served_csp(&cache, "buf-1"), build_document_csp(true));

    preview_set_scripts_inner(&store.lock().expect("store"), "buf-1", false).expect("off");
    assert!(matches!(render(), CachedRender::Rendered { .. }));
    assert_eq!(served_csp(&cache, "buf-1"), build_document_csp(false));
}

#[test]
fn a_render_of_a_file_the_store_does_not_hold_is_served_with_scripts_off() {
    let (_dir, store) = store_with("buf-1");
    preview_set_scripts_inner(&store, "buf-1", true).expect("on");
    let store = Mutex::new(store);
    let registry = html_registry();
    let cache = RenderCache::new();

    let rendered = render_into_cache(&store, &registry, &cache, "no-such-buffer", html_request());

    assert!(matches!(rendered, CachedRender::Rendered { .. }));
    assert_eq!(
        served_csp(&cache, "no-such-buffer"),
        build_document_csp(false)
    );
}

#[test]
fn a_content_type_with_no_renderer_caches_nothing() {
    let (_dir, store) = store_with("buf-1");
    let store = Mutex::new(store);
    let registry = RwLock::new(ContentRendererRegistry::new());
    let cache = RenderCache::new();

    let rendered = render_into_cache(&store, &registry, &cache, "buf-1", html_request());

    assert!(matches!(rendered, CachedRender::NoRenderer));
    assert!(cache.get("buf-1").is_none());
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
