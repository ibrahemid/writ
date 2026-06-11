//! Shared preview theme — the fallback stylesheet, inlined.
//!
//! Renderers inline the base theme as a `<style>` block in the served
//! document rather than linking to `writ-preview://chrome/preview-base.css`.
//! Two reasons:
//!
//! 1. **It always applies.** A cross-scope `<link>` from the
//!    `writ-preview://document` iframe to the `writ-preview://chrome` scope
//!    is a separate sub-request that can silently fail to apply (and did, in
//!    L4 smoke — Markdown rendered unstyled). An inline `<style>` is covered
//!    by the document CSP's `style-src 'unsafe-inline'` and needs no second
//!    request.
//! 2. **Self-contained documents.** The lean model is offline agent output;
//!    a preview document that carries its own base styling has no external
//!    dependency to resolve.
//!
//! The chrome scope still exists for the genuinely-large bundled runtimes
//! (Mermaid in L5, KaTeX in L6) — those stay external scripts. Only the
//! small, styling-critical base CSS is inlined.

/// The bundled fallback stylesheet, compiled into the binary.
pub const PREVIEW_BASE_CSS: &str = include_str!("../../../assets/preview-base.css");

/// The base theme as an inline `<style>` element.
pub fn style_tag() -> String {
    format!("<style>{PREVIEW_BASE_CSS}</style>")
}

/// Same-origin URL of the bundled first-party preview bridge runtime.
pub const BRIDGE_URL: &str = "writ-preview://document/_assets/preview/bridge.js";

/// The first-party bridge as a `<script>` element loaded same-origin.
///
/// The bridge wires scroll-sync and in-preview find between the app shell and
/// the cross-origin preview iframe over `postMessage`. It is injected into
/// every rendered document (loaded under the document CSP's
/// `script-src 'self' writ-preview:`); when the scripts kill switch is off it
/// is blocked along with all other scripts and the features degrade silently.
pub fn bridge_script_tag() -> String {
    format!("<script src=\"{BRIDGE_URL}\"></script>")
}

/// Wrap an HTML body fragment (e.g. Markdown output) in a complete,
/// self-contained document with the inlined base theme and a UTF-8 charset.
pub fn wrap_document(body_fragment: &str) -> String {
    wrap_document_with("", body_fragment, "")
}

/// Like [`wrap_document`], but injects extra `<head>` content (e.g. a runtime
/// stylesheet `<link>`) and extra content immediately before `</body>` (e.g. a
/// runtime `<script>`). Renderers that enhance the document with a bundled
/// runtime (Mermaid in L5, KaTeX in L6) use this to load those runtimes from
/// the same-origin `writ-preview://document/_assets/` route.
pub fn wrap_document_with(head_extra: &str, body_fragment: &str, body_end_extra: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n{style}\n{head_extra}\n</head>\n<body>\n{body_fragment}\n{body_end_extra}\n{bridge}\n</body>\n</html>",
        style = style_tag(),
        bridge = bridge_script_tag(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_css_is_nonempty_and_themed() {
        // Sanity: the compiled stylesheet carries the theme tokens, so an
        // empty or wrong file fails the build-time include here.
        assert!(PREVIEW_BASE_CSS.contains("--writ-preview-bg"));
        assert!(PREVIEW_BASE_CSS.len() > 1000);
    }

    #[test]
    fn style_tag_wraps_the_css() {
        let tag = style_tag();
        assert!(tag.starts_with("<style>"));
        assert!(tag.ends_with("</style>"));
        assert!(tag.contains("--writ-preview-bg"));
    }

    #[test]
    fn wrap_document_injects_the_preview_bridge_script_before_body_close() {
        // Every wrapped document (Markdown, standalone Mermaid) carries the
        // first-party bridge so scroll-sync / in-preview find work. It is a
        // same-origin _assets script, injected at the body tail.
        let doc = wrap_document("<p>x</p>");
        assert!(doc.contains("writ-preview://document/_assets/preview/bridge.js"));
        let script_idx = doc.find("/preview/bridge.js").unwrap();
        let body_close = doc.find("</body>").unwrap();
        assert!(script_idx < body_close, "bridge script must sit inside <body>");
    }

    #[test]
    fn wrap_document_is_a_full_self_contained_document() {
        let doc = wrap_document("<h1>hi</h1>");
        assert!(doc.contains("<!doctype html>"));
        assert!(doc.contains("<meta charset=\"utf-8\">"));
        assert!(doc.contains("<style>"));
        assert!(doc.contains("<h1>hi</h1>"));
        // The body fragment sits after the inlined style.
        let style_idx = doc.find("<style>").unwrap();
        let body_idx = doc.find("<h1>").unwrap();
        assert!(style_idx < body_idx);
    }
}
