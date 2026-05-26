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
//! HTML (reload is the only push mechanism compatible with the locked-down
//! `script-src 'none'` document CSP).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{Manager, Runtime, UriSchemeContext};
use writ_core::preview::PreviewPolicy;

use super::csp::build_csp;
use super::protocol::{parse, record, Disposition, PreviewScope, RefusalReason, RequestRecord};
use crate::state::AppState;

/// A document rendered and ready to serve.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedDoc {
    /// Fully-resolved HTML to serve for the document scope.
    pub html: String,
    /// Policy the document was rendered under (drives the CSP header).
    pub policy: PreviewPolicy,
}

/// Per-buffer cache of the most recently rendered document.
///
/// Keyed by buffer id: the rendered HTML does not differ per window in
/// Phase 2 (policy is always SAFE until Phase 3). The key widens to
/// `(WindowId, BufferId)` when per-window session policies land.
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
        self.docs
            .lock()
            .expect("render cache mutex poisoned")
            .insert(buffer_id.into(), doc);
    }

    /// Fetch the rendered document for `buffer_id`.
    pub fn get(&self, buffer_id: &str) -> Option<RenderedDoc> {
        self.docs
            .lock()
            .expect("render cache mutex poisoned")
            .get(buffer_id)
            .cloned()
    }

    /// Drop the cached document for `buffer_id` (on preview close).
    pub fn evict(&self, buffer_id: &str) {
        self.docs
            .lock()
            .expect("render cache mutex poisoned")
            .remove(buffer_id);
    }
}

/// One header on an outgoing response.
pub type Header = (&'static str, String);

/// Resolved response the handler will send.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedResponse {
    /// HTTP status code.
    pub status: u16,
    /// Body bytes.
    pub body: Vec<u8>,
    /// Headers, including Content-Type, the CSP, and nosniff.
    pub headers: Vec<Header>,
    /// Disposition recorded for diagnostics / the verification suite.
    pub disposition: Disposition,
}

impl ResolvedResponse {
    fn refused(reason: RefusalReason) -> Self {
        Self {
            status: 403,
            body: b"refused".to_vec(),
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
            body: b"not found".to_vec(),
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
/// `document_lookup` returns the rendered document for a buffer id;
/// `chrome_asset` returns `(bytes, mime)` for a bundled chrome asset path.
/// Both are closures so this function stays free of Tauri and `AppState`.
pub fn resolve(
    url: &str,
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
                body: bytes.to_vec(),
                headers: vec![
                    ("Content-Type", mime.to_string()),
                    ("X-Content-Type-Options", "nosniff".to_string()),
                    (
                        "Content-Security-Policy",
                        build_csp(PreviewScope::Chrome, PreviewPolicy::Safe),
                    ),
                    ("Cross-Origin-Resource-Policy", "same-origin".to_string()),
                ],
                disposition: Disposition::Allowed,
            },
            None => ResolvedResponse::not_found(),
        },
        PreviewScope::Document => {
            // The document path's first segment is the buffer id.
            let buffer_id = parsed.path.split('/').next().unwrap_or("");
            match document_lookup(buffer_id) {
                Some(doc) => ResolvedResponse {
                    status: 200,
                    body: doc.html.into_bytes(),
                    headers: vec![
                        ("Content-Type", "text/html; charset=utf-8".to_string()),
                        ("X-Content-Type-Options", "nosniff".to_string()),
                        (
                            "Content-Security-Policy",
                            build_csp(PreviewScope::Document, doc.policy),
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

    let resolved = resolve(
        &url,
        |id| state.preview_render_cache.get(id),
        chrome_asset,
    );

    record(record_for(&url, &resolved));

    let mut builder = tauri::http::Response::builder().status(resolved.status);
    for (key, value) in &resolved.headers {
        builder = builder.header(*key, value);
    }
    builder
        .body(Cow::Owned(resolved.body))
        .expect("preview response is always well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> RenderedDoc {
        RenderedDoc {
            html: html.to_string(),
            policy: PreviewPolicy::Safe,
        }
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
    fn chrome_stylesheet_is_served_with_css_mime_and_chrome_csp() {
        let r = resolve(
            "writ-preview://chrome/preview-base.css",
            |_| None,
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
        let r = resolve("writ-preview://chrome/nope.css", |_| None, chrome_asset);
        assert_eq!(r.status, 404);
    }

    #[test]
    fn document_serves_cached_html_with_document_csp_and_nosniff() {
        let cache = RenderCache::new();
        cache.put("buf-1", doc("<h1>rendered</h1>"));
        let r = resolve(
            "writ-preview://document/buf-1",
            |id| cache.get(id),
            chrome_asset,
        );
        assert_eq!(r.status, 200);
        assert_eq!(String::from_utf8(r.body).unwrap(), "<h1>rendered</h1>");
        assert!(r.headers.iter().any(|(k, v)| *k == "X-Content-Type-Options" && v == "nosniff"));
        let csp = r
            .headers
            .iter()
            .find(|(k, _)| *k == "Content-Security-Policy")
            .unwrap();
        // Document SAFE policy blocks scripts.
        assert!(csp.1.contains("script-src 'none'"));
    }

    #[test]
    fn document_with_trailing_path_keys_on_first_segment() {
        let cache = RenderCache::new();
        cache.put("buf-1", doc("<p>x</p>"));
        let r = resolve(
            "writ-preview://document/buf-1/index.html",
            |id| cache.get(id),
            chrome_asset,
        );
        assert_eq!(r.status, 200);
    }

    #[test]
    fn uncached_document_is_404() {
        let r = resolve("writ-preview://document/ghost", |_| None, chrome_asset);
        assert_eq!(r.status, 404);
    }

    #[test]
    fn traversal_is_refused_403() {
        let r = resolve(
            "writ-preview://document/../chrome/preview-base.css",
            |_| None,
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
        let r = resolve("https://evil/", |_| None, chrome_asset);
        assert_eq!(r.status, 403);
    }
}
