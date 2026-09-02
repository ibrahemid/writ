//! `writ-preview://` protocol handler — response resolution + render cache.
//!
//! The substrate (ADR-009 A1) makes Writ the network boundary: every
//! request is parsed by [`super::protocol::parse`], then resolved here.
//! The resolution core ([`resolve`]) is pure — it takes the parsed request
//! plus lookup closures and returns the bytes, MIME, and headers to send —
//! so it is unit-tested without a running Tauri app. The Tauri glue
//! ([`register`]) wires the closures to `AppState` and the bundled chrome
//! assets.
//!
//! The preview renders the **live** buffer text, not on-disk content: the
//! frontend debounces keystrokes into `preview_render`, which runs the
//! renderer and stores the HTML in the [`RenderCache`]; the document-scope
//! response serves from that cache, and the webview reloads to pick up new
//! HTML (reload is the push mechanism that works with or without scripts).
//!
//! The CSP header is computed at serve time, not render time: the document
//! CSP depends only on the app-level `preview.run_scripts` kill switch
//! (lean scope — one fixed policy, no per-document trust state).

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use tauri::{Manager, Runtime, UriSchemeContext};
use tracing::warn;
use writ_core::preview::protocol::{
    resolve_asset, sniff_image_mime, split_asset_request, AssetRequest,
};
use writ_core::preview::AssetScope;

use super::csp::{build_chrome_csp, build_document_csp};
use super::protocol::{parse, record, Disposition, PreviewScope, RefusalReason, RequestRecord};
use crate::poison::recover_poison;
use crate::state::AppState;

/// Largest note asset served inline (ADR-035). Past it the preview shows a
/// placeholder instead of pushing tens of megabytes into the webview.
const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;

/// Content type for both a served SVG and the placeholder.
const SVG_MIME: &str = "image/svg+xml";

/// Policy attached to every SVG response.
///
/// An SVG loaded through `<img>` is a passive image and runs no script by
/// the HTML spec; this covers the other way in, a direct navigation to the
/// asset URL, where the document would otherwise be live. ADR-035.
const SVG_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

/// A document rendered and ready to serve.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedDoc {
    /// Fully-resolved HTML to serve for the document scope.
    pub html: String,
    /// Roots the document's embedded files resolve under, carried from the
    /// render so an incoming asset URL is re-checked against the same two
    /// folders rather than trusted. `None` for a buffer with no file on
    /// disk, which serves no assets at all.
    pub assets: Option<AssetScope>,
}

/// Per-buffer cache of the most recently rendered document.
///
/// Keyed by buffer id: the rendered HTML is identical across windows (one
/// fixed policy, no per-window trust state in the lean scope).
#[derive(Default)]
pub struct RenderCache {
    docs: Mutex<HashMap<String, RenderedDoc>>,
}

impl RenderCache {
    /// Construct an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store (or replace) the rendered document for `buffer_id`.
    pub fn put(&self, buffer_id: impl Into<String>, doc: RenderedDoc) {
        recover_poison(self.docs.lock(), "preview::render_cache::put")
            .insert(buffer_id.into(), doc);
    }

    /// Fetch the rendered document for `buffer_id`.
    pub fn get(&self, buffer_id: &str) -> Option<RenderedDoc> {
        recover_poison(self.docs.lock(), "preview::render_cache::get")
            .get(buffer_id)
            .cloned()
    }

    /// Drop the cached document for `buffer_id` (on preview close).
    pub fn evict(&self, buffer_id: &str) {
        recover_poison(self.docs.lock(), "preview::render_cache::evict").remove(buffer_id);
    }
}

/// One header on an outgoing response.
pub type Header = (&'static str, String);

/// Resolved response the handler will send.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedResponse {
    /// HTTP status code.
    pub status: u16,
    /// Body bytes. Static assets (the chrome / `_assets` runtimes) borrow
    /// straight from the binary so a multi-MB runtime is served without a
    /// per-request copy on the live-render path; rendered documents own theirs.
    pub body: Cow<'static, [u8]>,
    /// Headers, including Content-Type, the CSP, and nosniff.
    pub headers: Vec<Header>,
    /// Disposition recorded for diagnostics / the verification suite.
    pub disposition: Disposition,
}

impl ResolvedResponse {
    fn refused(reason: RefusalReason) -> Self {
        Self {
            status: 403,
            body: Cow::Borrowed(b"refused"),
            headers: vec![
                ("Content-Type", "text/plain; charset=utf-8".to_string()),
                ("X-Content-Type-Options", "nosniff".to_string()),
            ],
            disposition: Disposition::Refused(reason),
        }
    }

    fn not_found() -> Self {
        Self {
            status: 404,
            body: Cow::Borrowed(b"not found"),
            headers: vec![
                ("Content-Type", "text/plain; charset=utf-8".to_string()),
                ("X-Content-Type-Options", "nosniff".to_string()),
            ],
            // A 404 is an allowed-but-empty resolution, not a refusal.
            disposition: Disposition::Allowed,
        }
    }
}

/// Resolve a request URL, including the note-asset route.
///
/// The route is split out from [`resolve`] because it is the one response
/// that reads a file off disk: everything else is served from the render
/// cache or from bytes compiled into the binary. `is_dataless` is injected
/// so the no-read rule for a provider placeholder is testable without one.
pub fn resolve_with_assets(
    url: &str,
    scripts_enabled: bool,
    document_lookup: impl Fn(&str) -> Option<RenderedDoc>,
    chrome_asset: impl Fn(&str) -> Option<(&'static [u8], &'static str)>,
    is_dataless: &dyn Fn(&Path) -> bool,
) -> ResolvedResponse {
    if let Ok(parsed) = parse(url) {
        if parsed.scope == PreviewScope::Document {
            if let Some(request) = split_asset_request(&parsed.path) {
                return resolve_note_asset(request, &document_lookup, is_dataless);
            }
        }
    }
    resolve(url, scripts_enabled, document_lookup, chrome_asset)
}

/// Serve one file embedded in a note.
///
/// Containment is decided by `writ_core`, over the roots the render recorded,
/// on the canonical path. What is left here is the refusal to read: a file
/// the provider has not downloaded, one past the size cap, and anything whose
/// leading bytes are not an image all come back as a placeholder naming the
/// file, so a note reads the same whether or not its images are at hand.
fn resolve_note_asset(
    request: AssetRequest<'_>,
    document_lookup: &impl Fn(&str) -> Option<RenderedDoc>,
    is_dataless: &dyn Fn(&Path) -> bool,
) -> ResolvedResponse {
    let file_name = request.relative.rsplit('/').next().unwrap_or_default();
    let Some(scope) = document_lookup(request.buffer_id).and_then(|doc| doc.assets) else {
        return ResolvedResponse::not_found();
    };
    let path = match resolve_asset(&scope.notes_root, &scope.note_dir, request) {
        Ok(path) => path,
        Err(reason) => {
            warn!(
                reason = reason.as_str(),
                buffer_id = request.buffer_id,
                relative = request.relative,
                "preview asset refused"
            );
            return ResolvedResponse::refused(reason);
        }
    };
    // C1: a file the provider reports as not downloaded is named, never
    // opened. Reading it would pull it down mid-scroll.
    if is_dataless(&path) {
        return asset_placeholder(file_name);
    }
    let Ok(metadata) = std::fs::metadata(&path) else {
        return asset_placeholder(file_name);
    };
    if !metadata.is_file() || metadata.len() > MAX_ASSET_BYTES {
        return asset_placeholder(file_name);
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return asset_placeholder(file_name);
    };
    // The extension is what a file's author controls; the leading bytes are
    // not. A `.png` holding markup sniffs as nothing and is never served.
    let Some(mime) = sniff_image_mime(&bytes) else {
        return asset_placeholder(file_name);
    };
    let mut headers = vec![
        ("Content-Type", mime.to_string()),
        ("X-Content-Type-Options", "nosniff".to_string()),
        // Keyed on mtime and size, so editing an image in place changes the
        // validator and the webview takes the new bytes.
        ("ETag", asset_etag(&metadata)),
        ("Cache-Control", "no-cache".to_string()),
    ];
    if mime == SVG_MIME {
        headers.push(("Content-Security-Policy", SVG_CSP.to_string()));
    }
    ResolvedResponse {
        status: 200,
        body: Cow::Owned(bytes),
        headers,
        disposition: Disposition::Allowed,
    }
}

/// Entity tag for a served asset: modification time and size.
fn asset_etag(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    format!("\"{modified}-{}\"", metadata.len())
}

/// The image served in place of one that cannot be: a quiet frame carrying
/// the file's name, under the same policy a real SVG gets.
fn asset_placeholder(file_name: &str) -> ResolvedResponse {
    ResolvedResponse {
        status: 200,
        body: Cow::Owned(placeholder_svg(file_name).into_bytes()),
        headers: vec![
            ("Content-Type", SVG_MIME.to_string()),
            ("X-Content-Type-Options", "nosniff".to_string()),
            ("Cache-Control", "no-cache".to_string()),
            ("Content-Security-Policy", SVG_CSP.to_string()),
        ],
        disposition: Disposition::Allowed,
    }
}

fn placeholder_svg(file_name: &str) -> String {
    let name = escape_xml(file_name);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"64\" \
viewBox=\"0 0 320 64\" role=\"img\" aria-label=\"{name}\">\
<rect x=\"0.5\" y=\"0.5\" width=\"319\" height=\"63\" rx=\"6\" fill=\"none\" \
stroke=\"#c9c6c0\" stroke-dasharray=\"4 4\"/>\
<text x=\"16\" y=\"37\" font-family=\"system-ui, -apple-system, sans-serif\" \
font-size=\"13\" fill=\"#8a867f\">{name}</text></svg>"
    )
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Resolve a request URL into a response.
///
/// `scripts_enabled` is the app-level kill switch, applied to the document
/// CSP at serve time. `document_lookup` returns the rendered document for a
/// buffer id; `chrome_asset` returns `(bytes, mime)` for a bundled chrome
/// asset path. The closures keep this function free of Tauri and `AppState`.
///
/// The note-asset route is handled ahead of this, in [`resolve_with_assets`];
/// this function stays free of I/O.
pub fn resolve(
    url: &str,
    scripts_enabled: bool,
    document_lookup: impl Fn(&str) -> Option<RenderedDoc>,
    chrome_asset: impl Fn(&str) -> Option<(&'static [u8], &'static str)>,
) -> ResolvedResponse {
    let parsed = match parse(url) {
        Ok(p) => p,
        Err(reason) => return ResolvedResponse::refused(reason),
    };

    match parsed.scope {
        PreviewScope::Chrome => match chrome_asset(&parsed.path) {
            Some((bytes, mime)) => ResolvedResponse {
                status: 200,
                body: Cow::Borrowed(bytes),
                headers: vec![
                    ("Content-Type", mime.to_string()),
                    ("X-Content-Type-Options", "nosniff".to_string()),
                    ("Content-Security-Policy", build_chrome_csp()),
                    ("Cross-Origin-Resource-Policy", "same-origin".to_string()),
                ],
                disposition: Disposition::Allowed,
            },
            None => ResolvedResponse::not_found(),
        },
        PreviewScope::Document => {
            // Reserved same-origin asset route. The bundled host runtimes
            // (Mermaid, KaTeX) are served under the document origin so the
            // preview iframe loads them same-origin — a cross-origin
            // `writ-preview://chrome` subresource is refused by that scope's
            // `Cross-Origin-Resource-Policy: same-origin`. The path after
            // `_assets/` keys the same host-owned chrome-asset table; the URL
            // parser has already rejected any traversal. Buffer ids are
            // generated UUIDs and never collide with the `_assets` prefix.
            if let Some(asset_path) = parsed.path.strip_prefix("_assets/") {
                return match chrome_asset(asset_path) {
                    // Same-origin to the document, so no Cross-Origin-Resource-
                    // Policy header is needed (or wanted) here — its omission is
                    // intentional, unlike the chrome scope which sets it.
                    Some((bytes, mime)) => ResolvedResponse {
                        status: 200,
                        body: Cow::Borrowed(bytes),
                        headers: vec![
                            ("Content-Type", mime.to_string()),
                            ("X-Content-Type-Options", "nosniff".to_string()),
                        ],
                        disposition: Disposition::Allowed,
                    },
                    None => ResolvedResponse::not_found(),
                };
            }
            // The document path's first segment is the buffer id.
            let buffer_id = parsed.path.split('/').next().unwrap_or("");
            match document_lookup(buffer_id) {
                Some(doc) => ResolvedResponse {
                    status: 200,
                    body: Cow::Owned(doc.html.into_bytes()),
                    headers: vec![
                        ("Content-Type", "text/html; charset=utf-8".to_string()),
                        ("X-Content-Type-Options", "nosniff".to_string()),
                        (
                            "Content-Security-Policy",
                            build_document_csp(scripts_enabled),
                        ),
                    ],
                    disposition: Disposition::Allowed,
                },
                None => ResolvedResponse::not_found(),
            }
        }
    }
}

/// MIME type for a bundled chrome asset, by extension. Chrome assets are a
/// fixed, host-owned set; an unknown extension is treated as octet-stream.
pub fn chrome_mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("html") => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The bundled chrome asset table. Phase 2 ships the fallback stylesheet;
/// later phases add the Mermaid / KaTeX / PDF.js runtimes.
pub fn chrome_asset(path: &str) -> Option<(&'static [u8], &'static str)> {
    match path {
        "preview-base.css" => Some((
            include_bytes!("../../assets/preview-base.css"),
            chrome_mime("preview-base.css"),
        )),
        "blank" | "blank.html" => Some((
            b"<!doctype html><title>writ</title>",
            chrome_mime("blank.html"),
        )),
        // First-party scroll-sync / in-preview find bridge, served same-origin
        // under the document scope (injected into every rendered document).
        "preview/bridge.js" => Some((
            include_bytes!("../../assets/preview/bridge.js"),
            chrome_mime("bridge.js"),
        )),
        // L5 — bundled offline Mermaid runtime (single-file IIFE).
        "mermaid/mermaid.min.js" => Some((
            include_bytes!("../../assets/mermaid/mermaid.min.js"),
            chrome_mime("mermaid.min.js"),
        )),
        // L6 — bundled offline KaTeX runtime (js, css, woff2 fonts). The whole
        // `katex/` subtree is owned by the renderer module's asset table.
        p if p.starts_with("katex/") => super::renderers::katex::asset(&p["katex/".len()..])
            .map(|bytes| (bytes, chrome_mime(p))),
        _ => None,
    }
}

/// Build the request record for the diagnostics recorder.
pub fn record_for(url: &str, response: &ResolvedResponse) -> RequestRecord {
    RequestRecord {
        url: url.to_string(),
        disposition: response.disposition.clone(),
    }
}

/// Tauri `register_uri_scheme_protocol` glue for `writ-preview://`.
///
/// Resolves the request through the pure [`resolve`] core wired to the
/// `AppState` render cache and the bundled chrome assets, records the
/// disposition for the verification suite (debug builds), and serializes
/// the result into a Tauri HTTP response.
pub fn serve<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let url = request.uri().to_string();
    let state = ctx.app_handle().state::<AppState>();

    let scripts_enabled = state
        .config
        .lock()
        .map(|c| c.preview.run_scripts)
        .unwrap_or(true);

    let resolved = resolve_with_assets(
        &url,
        scripts_enabled,
        |id| state.preview_render_cache.get(id),
        chrome_asset,
        &writ_storage::notes_index::is_dataless,
    );

    record(record_for(&url, &resolved));

    build_http_response(resolved)
}

/// Serialize a [`ResolvedResponse`] into the Tauri HTTP response the webview
/// receives. Factored out of [`serve`] so the header attachment — in
/// particular that the locked CSP lands on the response object, not just in
/// `build_document_csp`'s return value — is testable without a running app.
pub fn build_http_response(
    resolved: ResolvedResponse,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let mut builder = tauri::http::Response::builder().status(resolved.status);
    for (key, value) in &resolved.headers {
        builder = builder.header(*key, value);
    }
    builder
        .body(resolved.body)
        .expect("preview response is always well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scripts default on in the lean scope; tests pass the flag explicitly.
    const SCRIPTS_ON: bool = true;

    fn doc(html: &str) -> RenderedDoc {
        RenderedDoc {
            html: html.to_string(),
            assets: None,
        }
    }

    fn no_doc(_: &str) -> Option<RenderedDoc> {
        None
    }

    #[test]
    fn render_cache_put_get_evict() {
        let cache = RenderCache::new();
        assert_eq!(cache.get("b1"), None);
        cache.put("b1", doc("<p>hi</p>"));
        assert_eq!(cache.get("b1").unwrap().html, "<p>hi</p>");
        cache.evict("b1");
        assert_eq!(cache.get("b1"), None);
    }

    #[test]
    fn render_cache_isolates_buffers() {
        // The frontend keys every render on the buffer id; the cache must keep
        // those slots strictly independent so one buffer's HTML can never be
        // served under another's id. This is the host-side invariant the #97
        // stale-cache flash fix relies on: the misattribution it guards against
        // is a frontend coordination bug, never the cache conflating ids.
        let cache = RenderCache::new();
        cache.put("a", doc("<body>A</body>"));
        cache.put("b", doc("<body>B</body>"));
        assert_eq!(cache.get("a").unwrap().html, "<body>A</body>");
        assert_eq!(cache.get("b").unwrap().html, "<body>B</body>");

        // Re-rendering one slot leaves the other untouched.
        cache.put("a", doc("<body>A2</body>"));
        assert_eq!(cache.get("a").unwrap().html, "<body>A2</body>");
        assert_eq!(cache.get("b").unwrap().html, "<body>B</body>");

        // Evicting one slot (pane close) leaves the other intact.
        cache.evict("a");
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b").unwrap().html, "<body>B</body>");
    }

    #[test]
    fn chrome_stylesheet_is_served_with_css_mime_and_chrome_csp() {
        let r = resolve(
            "writ-preview://chrome/preview-base.css",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(r.status, 200);
        assert!(!r.body.is_empty());
        let ct = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Type")
            .unwrap();
        assert!(ct.1.starts_with("text/css"));
        let csp = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Security-Policy")
            .unwrap();
        assert!(csp.1.contains("script-src 'self'"));
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| *k == "X-Content-Type-Options" && v == "nosniff"));
    }

    #[test]
    fn unknown_chrome_asset_is_404() {
        let r = resolve(
            "writ-preview://chrome/nope.css",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(r.status, 404);
    }

    #[test]
    fn assets_route_serves_the_preview_bridge_same_origin_with_js_mime() {
        // The first-party bridge is served under the document origin so the
        // preview iframe loads it same-origin (no cross-scope CORP refusal),
        // exactly like the Mermaid/KaTeX runtimes.
        let r = resolve(
            "writ-preview://document/_assets/preview/bridge.js",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(r.status, 200);
        assert!(!r.body.is_empty());
        let ct = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Type")
            .unwrap();
        assert!(ct.1.starts_with("application/javascript"));
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| *k == "X-Content-Type-Options" && v == "nosniff"));
        assert_eq!(r.disposition, Disposition::Allowed);
    }

    #[test]
    fn preview_bridge_runtime_requires_no_eval() {
        // The document CSP grants no 'unsafe-eval'; the bridge must never
        // depend on eval / the Function constructor. Static pin guarding a
        // future edit from silently reintroducing that dependency.
        let (bytes, _) = chrome_asset("preview/bridge.js").unwrap();
        let src = std::str::from_utf8(bytes).expect("bridge is valid utf-8");
        assert!(!src.contains("eval("), "bridge must not use eval()");
        assert!(
            !src.contains("new Function("),
            "bridge must not use new Function()"
        );
    }

    #[test]
    fn assets_route_serves_runtime_same_origin_under_document_scope() {
        // The bundled runtimes are reachable under the document origin so the
        // preview iframe loads them same-origin (no cross-scope CORP refusal).
        let r = resolve(
            "writ-preview://document/_assets/mermaid/mermaid.min.js",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(r.status, 200);
        assert!(!r.body.is_empty());
        let ct = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Type")
            .unwrap();
        assert!(ct.1.starts_with("application/javascript"));
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| *k == "X-Content-Type-Options" && v == "nosniff"));
        assert_eq!(r.disposition, Disposition::Allowed);
    }

    #[test]
    fn assets_route_serves_katex_css_and_fonts_with_correct_mime() {
        // The KaTeX stylesheet and its woff2 fonts are reachable same-origin so
        // the relative `url(fonts/…)` in the CSS resolves under `_assets/katex/`.
        let css = resolve(
            "writ-preview://document/_assets/katex/katex.min.css",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(css.status, 200);
        let ct = css
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Type")
            .unwrap();
        assert!(ct.1.starts_with("text/css"));

        let font = resolve(
            "writ-preview://document/_assets/katex/fonts/KaTeX_Main-Regular.woff2",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(font.status, 200);
        assert!(!font.body.is_empty());
        let ct = font
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Type")
            .unwrap();
        assert_eq!(ct.1, "font/woff2");
    }

    #[test]
    fn assets_route_unknown_is_404() {
        let r = resolve(
            "writ-preview://document/_assets/nope.js",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(r.status, 404);
    }

    #[test]
    fn assets_prefix_is_reserved_not_treated_as_a_buffer_id() {
        // A document request whose first segment is `_assets` must route to
        // the asset table, never to a buffer lookup. With an unknown asset it
        // is a 404, but it must not be served as document HTML.
        let cache = RenderCache::new();
        cache.put("_assets", doc("<h1>should never serve</h1>"));
        let r = resolve(
            "writ-preview://document/_assets/ghost.js",
            SCRIPTS_ON,
            |id| cache.get(id),
            chrome_asset,
        );
        assert_eq!(r.status, 404);
    }

    #[test]
    fn vendored_mermaid_runtime_requires_no_unsafe_eval() {
        // The document CSP grants no 'unsafe-eval'. The bundled IIFE must not
        // depend on eval / the Function constructor; this static pin guards a
        // future re-vendor from silently reintroducing that dependency. (The
        // ultimate proof is behavioral — a diagram rendering under the locked
        // CSP with zero violations — but this is the cheap in-repo guard.)
        let (bytes, _) = chrome_asset("mermaid/mermaid.min.js").unwrap();
        let src = std::str::from_utf8(bytes).expect("runtime is valid utf-8");

        assert!(
            !src.contains("new Function("),
            "runtime must not use new Function()"
        );
        assert!(!src.contains("eval("), "runtime must not use eval()");

        // Bare `Function(` calls are allowed only as the standard
        // `Function("return this")()` global-object idiom, which never runs in
        // a webview (an earlier `self`/`globalThis` operand short-circuits).
        // Anything else — a `Function(<code>)` constructor — would need
        // 'unsafe-eval' and must fail this guard. Method names like
        // `parseFunction(` are excluded by requiring a non-identifier prefix.
        for (idx, _) in src.match_indices("Function(") {
            let prev = src[..idx].chars().next_back().unwrap_or(' ');
            if prev == '.' || prev == '_' || prev == '$' || prev.is_ascii_alphanumeric() {
                continue; // part of an identifier, not the bare constructor
            }
            assert!(
                src[idx..].starts_with("Function(\"return this\")"),
                "unexpected Function constructor at byte {idx}",
            );
        }
    }

    #[test]
    fn document_serves_cached_html_with_document_csp_and_nosniff() {
        let cache = RenderCache::new();
        cache.put("buf-1", doc("<h1>rendered</h1>"));
        let r = resolve(
            "writ-preview://document/buf-1",
            SCRIPTS_ON,
            |id| cache.get(id),
            chrome_asset,
        );
        assert_eq!(r.status, 200);
        assert_eq!(
            String::from_utf8(r.body.into_owned()).unwrap(),
            "<h1>rendered</h1>"
        );
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| *k == "X-Content-Type-Options" && v == "nosniff"));
        let csp = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Security-Policy")
            .unwrap();
        // Scripts on → the document CSP permits inline + self + writ-preview:.
        assert!(csp
            .1
            .contains("script-src 'unsafe-inline' 'self' writ-preview:"));
    }

    #[test]
    fn document_csp_blocks_scripts_when_kill_switch_off() {
        let cache = RenderCache::new();
        cache.put("buf-1", doc("<h1>x</h1>"));
        let r = resolve(
            "writ-preview://document/buf-1",
            false,
            |id| cache.get(id),
            chrome_asset,
        );
        let csp = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Security-Policy")
            .unwrap();
        assert!(csp.1.contains("script-src 'none'"));
    }

    #[test]
    fn locked_csp_is_attached_to_the_http_response_object() {
        // The L2 requirement: prove the CSP is on the actual response the
        // webview receives, not merely that build_document_csp returns the
        // right string. Resolve a document request, build the real
        // tauri::http::Response, and read the header back off it.
        use crate::preview::csp::build_document_csp;

        let cache = RenderCache::new();
        cache.put("buf-1", doc("<h1>x</h1>"));

        for scripts_enabled in [true, false] {
            let resolved = resolve(
                "writ-preview://document/buf-1",
                scripts_enabled,
                |id| cache.get(id),
                chrome_asset,
            );
            let response = build_http_response(resolved);
            let header = response
                .headers()
                .get("Content-Security-Policy")
                .expect("CSP header attached to the response")
                .to_str()
                .unwrap();
            assert_eq!(header, build_document_csp(scripts_enabled));
            assert_eq!(
                response.headers().get("X-Content-Type-Options").unwrap(),
                "nosniff"
            );
        }
    }

    #[test]
    fn document_with_trailing_path_keys_on_first_segment() {
        let cache = RenderCache::new();
        cache.put("buf-1", doc("<p>x</p>"));
        let r = resolve(
            "writ-preview://document/buf-1/index.html",
            SCRIPTS_ON,
            |id| cache.get(id),
            chrome_asset,
        );
        assert_eq!(r.status, 200);
    }

    #[test]
    fn uncached_document_is_404() {
        let r = resolve(
            "writ-preview://document/ghost",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(r.status, 404);
    }

    #[test]
    fn traversal_is_refused_403() {
        let r = resolve(
            "writ-preview://document/../chrome/preview-base.css",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(r.status, 403);
        assert!(matches!(
            r.disposition,
            Disposition::Refused(RefusalReason::TraversalAttempt)
        ));
    }

    #[test]
    fn wrong_scheme_is_refused() {
        let r = resolve("https://evil/", SCRIPTS_ON, no_doc, chrome_asset);
        assert_eq!(r.status, 403);
    }
}

#[cfg(test)]
mod asset_tests {
    use super::*;
    use std::path::PathBuf;

    const SCRIPTS_ON: bool = true;
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

    fn never_dataless(_: &Path) -> bool {
        false
    }

    fn header<'a>(r: &'a ResolvedResponse, key: &str) -> Option<&'a str> {
        r.headers
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    /// A notes folder holding one note in `daily/` and a set of fixture
    /// files under `attachments/`, plus the render-cache entry the asset
    /// route looks the scope up through.
    struct Fixture {
        _guard: tempfile::TempDir,
        notes: PathBuf,
        note_dir: PathBuf,
        cache: RenderCache,
    }

    impl Fixture {
        fn new() -> Self {
            let guard = tempfile::tempdir().unwrap();
            let notes = guard.path().join("Writ");
            let note_dir = notes.join("daily");
            std::fs::create_dir_all(&note_dir).unwrap();
            let attachments = notes.join("attachments");
            std::fs::create_dir_all(&attachments).unwrap();
            std::fs::write(attachments.join("a.png"), PNG).unwrap();
            std::fs::write(attachments.join("b.jpg"), [0xFF, 0xD8, 0xFF, 0xE0, 0x00]).unwrap();
            std::fs::write(attachments.join("c.gif"), b"GIF89a\x01\x00\x01\x00").unwrap();
            std::fs::write(
                attachments.join("d.svg"),
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>",
            )
            .unwrap();
            // The fixture that matters: markup wearing an image extension.
            std::fs::write(
                attachments.join("liar.png"),
                b"<!doctype html><html><script>steal()</script></html>",
            )
            .unwrap();

            let cache = RenderCache::new();
            cache.put(
                "buf-1",
                RenderedDoc {
                    html: "<p>note</p>".to_string(),
                    assets: Some(AssetScope {
                        notes_root: notes.clone(),
                        note_dir: note_dir.clone(),
                        buffer_id: "buf-1".to_string(),
                    }),
                },
            );
            Self {
                _guard: guard,
                notes,
                note_dir,
                cache,
            }
        }

        fn get(&self, relative: &str) -> ResolvedResponse {
            self.get_with(relative, &never_dataless)
        }

        fn get_with(
            &self,
            relative: &str,
            is_dataless: &dyn Fn(&Path) -> bool,
        ) -> ResolvedResponse {
            resolve_with_assets(
                &format!("writ-preview://document/_note-asset/buf-1/n/{relative}"),
                SCRIPTS_ON,
                |id| self.cache.get(id),
                chrome_asset,
                is_dataless,
            )
        }
    }

    #[test]
    fn serves_each_image_format_by_its_leading_bytes() {
        let f = Fixture::new();
        for (relative, mime) in [
            ("attachments/a.png", "image/png"),
            ("attachments/b.jpg", "image/jpeg"),
            ("attachments/c.gif", "image/gif"),
            ("attachments/d.svg", SVG_MIME),
        ] {
            let r = f.get(relative);
            assert_eq!(r.status, 200, "relative={relative}");
            assert_eq!(
                header(&r, "Content-Type"),
                Some(mime),
                "relative={relative}"
            );
            assert_eq!(header(&r, "X-Content-Type-Options"), Some("nosniff"));
            assert_eq!(r.disposition, Disposition::Allowed);
            assert!(!r.body.is_empty());
        }
    }

    #[test]
    fn an_extension_that_lies_about_its_bytes_is_never_served() {
        let f = Fixture::new();
        let r = f.get("attachments/liar.png");
        assert_eq!(r.status, 200);
        assert_eq!(header(&r, "Content-Type"), Some(SVG_MIME));
        let body = String::from_utf8(r.body.into_owned()).unwrap();
        assert!(body.contains("liar.png"));
        assert!(!body.contains("steal()"));
    }

    #[test]
    fn svg_is_served_under_a_policy_that_cannot_run_script() {
        let f = Fixture::new();
        let r = f.get("attachments/d.svg");
        let csp = header(&r, "Content-Security-Policy").unwrap();
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("sandbox"));
        assert!(!csp.contains("script-src 'self'"));
    }

    #[test]
    fn caching_headers_key_on_mtime_and_size() {
        let f = Fixture::new();
        let first = header(&f.get("attachments/a.png"), "ETag")
            .unwrap()
            .to_string();
        assert_eq!(
            header(&f.get("attachments/a.png"), "Cache-Control"),
            Some("no-cache")
        );

        // Same bytes, same validator.
        assert_eq!(
            header(&f.get("attachments/a.png"), "ETag").map(str::to_string),
            Some(first.clone())
        );

        // Edited in place: a longer file with a later mtime must not reuse it.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut edited = PNG.to_vec();
        edited.extend_from_slice(b"more pixels");
        std::fs::write(f.notes.join("attachments/a.png"), &edited).unwrap();
        assert_ne!(
            header(&f.get("attachments/a.png"), "ETag").map(str::to_string),
            Some(first)
        );
    }

    #[test]
    fn a_missing_file_is_named_in_a_placeholder() {
        let f = Fixture::new();
        let r = f.get("attachments/gone.png");
        assert_eq!(r.status, 200);
        assert_eq!(header(&r, "Content-Type"), Some(SVG_MIME));
        assert!(String::from_utf8(r.body.into_owned())
            .unwrap()
            .contains("gone.png"));
    }

    #[test]
    fn a_file_past_the_size_cap_is_a_placeholder() {
        let f = Fixture::new();
        let mut huge = PNG.to_vec();
        huge.resize((MAX_ASSET_BYTES + 1) as usize, 0);
        std::fs::write(f.notes.join("attachments/huge.png"), &huge).unwrap();
        let r = f.get("attachments/huge.png");
        assert_eq!(header(&r, "Content-Type"), Some(SVG_MIME));
        assert!(String::from_utf8(r.body.into_owned())
            .unwrap()
            .contains("huge.png"));
    }

    #[test]
    fn a_file_not_downloaded_yet_is_named_and_never_opened() {
        let f = Fixture::new();
        // Deny every read: if the route opened the file the test would see
        // the real bytes instead of the placeholder.
        let denied = std::cell::Cell::new(0usize);
        let is_dataless = |path: &Path| {
            denied.set(denied.get() + 1);
            path.ends_with("a.png")
        };
        let r = f.get_with("attachments/a.png", &is_dataless);
        assert_eq!(denied.get(), 1);
        assert_eq!(header(&r, "Content-Type"), Some(SVG_MIME));
        let body = String::from_utf8(r.body.into_owned()).unwrap();
        assert!(body.contains("a.png"));
        assert!(!body.contains("IHDR"));
    }

    #[test]
    fn a_reference_outside_the_roots_is_refused_not_served() {
        let f = Fixture::new();
        // The parser refuses the traversal spelling outright.
        let traversal = resolve_with_assets(
            "writ-preview://document/_note-asset/buf-1/n/../../../etc/hosts",
            SCRIPTS_ON,
            |id| f.cache.get(id),
            chrome_asset,
            &never_dataless,
        );
        assert_eq!(traversal.status, 403);
        assert_eq!(
            traversal.disposition,
            Disposition::Refused(RefusalReason::TraversalAttempt)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_refused() {
        let f = Fixture::new();
        let outside = f._guard.path().join("outside.png");
        std::fs::write(&outside, PNG).unwrap();
        std::os::unix::fs::symlink(&outside, f.notes.join("attachments/link.png")).unwrap();
        let r = f.get("attachments/link.png");
        assert_eq!(r.status, 403);
        assert_eq!(
            r.disposition,
            Disposition::Refused(RefusalReason::SymlinkRefused)
        );
    }

    #[test]
    fn an_unknown_root_token_is_not_an_asset_request() {
        let f = Fixture::new();
        // No root discriminator: the path is not an asset request at all, so
        // it falls through to the document lookup and 404s on the buffer id.
        let r = resolve_with_assets(
            "writ-preview://document/_note-asset/buf-1/x/attachments/a.png",
            SCRIPTS_ON,
            |id| f.cache.get(id),
            chrome_asset,
            &never_dataless,
        );
        assert_eq!(r.status, 404);
    }

    #[test]
    fn a_buffer_with_no_asset_scope_serves_nothing() {
        let cache = RenderCache::new();
        cache.put(
            "scratch",
            RenderedDoc {
                html: "<p>x</p>".to_string(),
                assets: None,
            },
        );
        let r = resolve_with_assets(
            "writ-preview://document/_note-asset/scratch/n/a.png",
            SCRIPTS_ON,
            |id| cache.get(id),
            chrome_asset,
            &never_dataless,
        );
        assert_eq!(r.status, 404);
    }

    #[test]
    fn the_bundled_runtime_route_is_untouched_by_the_asset_route() {
        let f = Fixture::new();
        let r = resolve_with_assets(
            "writ-preview://document/_assets/preview/bridge.js",
            SCRIPTS_ON,
            |id| f.cache.get(id),
            chrome_asset,
            &never_dataless,
        );
        assert_eq!(r.status, 200);
        assert!(header(&r, "Content-Type")
            .unwrap()
            .starts_with("application/javascript"));
        // And the document itself still serves from the same cache entry.
        let document = resolve_with_assets(
            "writ-preview://document/buf-1",
            SCRIPTS_ON,
            |id| f.cache.get(id),
            chrome_asset,
            &never_dataless,
        );
        assert_eq!(document.status, 200);
        assert_eq!(
            String::from_utf8(document.body.into_owned()).unwrap(),
            "<p>note</p>"
        );
        let _ = &f.note_dir;
    }
}
