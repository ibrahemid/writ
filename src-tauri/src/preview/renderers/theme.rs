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

/// Wrap an HTML body fragment (e.g. Markdown output) in a complete,
/// self-contained document with the inlined base theme and a UTF-8 charset.
pub fn wrap_document(body_fragment: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n{}\n</head>\n<body>\n{body_fragment}\n</body>\n</html>",
        style_tag()
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
