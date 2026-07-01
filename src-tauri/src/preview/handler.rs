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
use std::sync::Mutex;

use tauri::{Manager, Runtime, UriSchemeContext};

use super::csp::{build_chrome_csp, build_document_csp};
use super::protocol::{parse, record, Disposition, PreviewScope, RefusalReason, RequestRecord};
use crate::poison::recover_poison;
use crate::state::AppState;

/// A document rendered and ready to serve.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedDoc {
    /// Fully-resolved HTML to serve for the document scope.
    pub html: String,
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
        recover_poison(self.docs.lock(), "preview::render_cache::put").insert(buffer_id.into(), doc);
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

/// Resolve a request URL into a response.
///
/// `scripts_enabled` is the app-level kill switch, applied to the document
/// CSP at serve time. `document_lookup` returns the rendered document for a
/// buffer id; `chrome_asset` returns `(bytes, mime)` for a bundled chrome
/// asset path. The closures keep this function free of Tauri and `AppState`.
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
        "blank" | "blank.html" => Some((b"<!doctype html><title>writ</title>", chrome_mime("blank.html"))),
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

    let resolved = resolve(
        &url,
        scripts_enabled,
        |id| state.preview_render_cache.get(id),
        chrome_asset,
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
        let ct = r.headers.iter().find(|(k, _)| *k == "Content-Type").unwrap();
        assert!(ct.1.starts_with("text/css"));
        let csp = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Security-Policy")
            .unwrap();
        assert!(csp.1.contains("script-src 'self'"));
        assert!(r.headers.iter().any(|(k, v)| *k == "X-Content-Type-Options" && v == "nosniff"));
    }

    #[test]
    fn unknown_chrome_asset_is_404() {
        let r = resolve("writ-preview://chrome/nope.css", SCRIPTS_ON, no_doc, chrome_asset);
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
        let ct = r.headers.iter().find(|(k, _)| *k == "Content-Type").unwrap();
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
        assert!(!src.contains("new Function("), "bridge must not use new Function()");
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
        let ct = r.headers.iter().find(|(k, _)| *k == "Content-Type").unwrap();
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
        let ct = css.headers.iter().find(|(k, _)| *k == "Content-Type").unwrap();
        assert!(ct.1.starts_with("text/css"));

        let font = resolve(
            "writ-preview://document/_assets/katex/fonts/KaTeX_Main-Regular.woff2",
            SCRIPTS_ON,
            no_doc,
            chrome_asset,
        );
        assert_eq!(font.status, 200);
        assert!(!font.body.is_empty());
        let ct = font.headers.iter().find(|(k, _)| *k == "Content-Type").unwrap();
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

        assert!(!src.contains("new Function("), "runtime must not use new Function()");
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
        assert_eq!(String::from_utf8(r.body.into_owned()).unwrap(), "<h1>rendered</h1>");
        assert!(r.headers.iter().any(|(k, v)| *k == "X-Content-Type-Options" && v == "nosniff"));
        let csp = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Security-Policy")
            .unwrap();
        // Scripts on → the document CSP permits inline + self + writ-preview:.
        assert!(csp.1.contains("script-src 'unsafe-inline' 'self' writ-preview:"));
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
        let r = resolve("writ-preview://document/ghost", SCRIPTS_ON, no_doc, chrome_asset);
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
