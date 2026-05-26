//! HTML renderer — the first `ContentRenderer` (ADR-009 renderer roster).
//!
//! HTML is served **as-is**: author CSS is honored, the security boundary
//! is the per-webview CSP (Phase 3), not HTML rewriting. The renderer's
//! jobs are narrow:
//!
//! 1. Refuse documents over the renderer's hard ceiling.
//! 2. Decide whether the host fallback stylesheet applies — present only
//!    when the document brings no `<style>` and no `<link rel=stylesheet>`
//!    of its own (ADR-009 F2, "author styles win" made mechanical).
//! 3. Inject the fallback `<link>` into the served HTML when it applies.
//!
//! Parser warnings reported here are limited to what is cheaply detectable
//! without a full HTML parser; the webview's own permissive parser is the
//! renderer of record and surfaces its recovery warnings through the
//! rendered-event channel.

use writ_core::preview::{
    ContentRenderer, ContentTypeId, RenderError, RenderOutput, RenderRequest,
    RendererCapabilities,
};

/// `writ-preview://chrome/*` path the fallback stylesheet is served from.
pub const FALLBACK_STYLESHEET_URL: &str = "writ-preview://chrome/preview-base.css";

/// Hard ceiling above which the HTML renderer refuses. Mirrors ADR-009's
/// 50 MB surface refusal; the surface-level 1 MB / 5 MB thresholds are
/// applied by the caller in addition to this.
const MAX_SAFE_BYTES: u64 = 50 * 1024 * 1024;

/// The HTML content renderer.
pub struct HtmlRenderer;

impl HtmlRenderer {
    /// Content-type id this renderer registers under.
    pub fn content_type_id() -> ContentTypeId {
        ContentTypeId::new("html")
    }
}

impl ContentRenderer for HtmlRenderer {
    fn content_type(&self) -> ContentTypeId {
        Self::content_type_id()
    }

    fn capabilities(&self) -> RendererCapabilities {
        RendererCapabilities {
            supports_live_render: true,
            supports_print: true,
            max_safe_document_bytes: MAX_SAFE_BYTES,
        }
    }

    fn render(&self, request: RenderRequest) -> Result<RenderOutput, RenderError> {
        let bytes = request.buffer_text.len() as u64;
        if bytes > MAX_SAFE_BYTES {
            return Err(RenderError::DocumentTooLarge {
                bytes,
                limit: MAX_SAFE_BYTES,
            });
        }

        let has_own_styles = has_author_styles(&request.buffer_text);
        let used_fallback_stylesheet = !has_own_styles;

        let document_html = if used_fallback_stylesheet {
            inject_fallback_stylesheet(&request.buffer_text)
        } else {
            request.buffer_text.clone()
        };

        Ok(RenderOutput {
            document_html,
            used_fallback_stylesheet,
            parser_warnings: collect_cheap_warnings(&request.buffer_text),
        })
    }
}

/// Whether the document carries its own styling: an inline `<style>` block
/// or a `<link rel="stylesheet">`.
///
/// Conservative by construction: a false positive (deciding the document
/// has styles when it does not) merely withholds the fallback, which never
/// overrides author intent. The scan is case-insensitive.
fn has_author_styles(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    if lower.contains("<style") {
        return true;
    }
    // Look for a <link ...> tag whose attributes include rel=...stylesheet.
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("<link") {
        let tag_start = search_from + rel;
        let tag_end = lower[tag_start..]
            .find('>')
            .map(|i| tag_start + i)
            .unwrap_or(lower.len());
        let tag = &lower[tag_start..tag_end];
        if tag.contains("stylesheet") {
            return true;
        }
        search_from = tag_end + 1;
        if search_from >= lower.len() {
            break;
        }
    }
    false
}

/// Inject the fallback stylesheet `<link>` into the document.
///
/// Placement, in priority order: right after `<head>`, else right after
/// `<html ...>`, else prepended. The webview's permissive parser hoists a
/// leading `<link>` into the head regardless, so the prepend fallback is
/// safe; we still prefer the head for well-formed documents.
fn inject_fallback_stylesheet(html: &str) -> String {
    let link = format!(
        "<link rel=\"stylesheet\" href=\"{FALLBACK_STYLESHEET_URL}\">"
    );
    let lower = html.to_ascii_lowercase();

    if let Some(idx) = lower.find("<head>") {
        let insert_at = idx + "<head>".len();
        let mut out = String::with_capacity(html.len() + link.len());
        out.push_str(&html[..insert_at]);
        out.push_str(&link);
        out.push_str(&html[insert_at..]);
        return out;
    }

    if let Some(idx) = lower.find("<head") {
        // <head> with attributes: insert after the closing '>'.
        if let Some(gt) = lower[idx..].find('>') {
            let insert_at = idx + gt + 1;
            let mut out = String::with_capacity(html.len() + link.len());
            out.push_str(&html[..insert_at]);
            out.push_str(&link);
            out.push_str(&html[insert_at..]);
            return out;
        }
    }

    if let Some(idx) = lower.find("<html") {
        if let Some(gt) = lower[idx..].find('>') {
            let insert_at = idx + gt + 1;
            let mut out = String::with_capacity(html.len() + link.len());
            out.push_str(&html[..insert_at]);
            out.push_str(&link);
            out.push_str(&html[insert_at..]);
            return out;
        }
    }

    format!("{link}{html}")
}

/// Cheap, parser-free warnings. Deliberately conservative: only flags
/// conditions that are unambiguous without a real parse.
fn collect_cheap_warnings(html: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if html.trim().is_empty() {
        // Empty documents are handled by the surface's themed empty state;
        // no warning needed.
        return warnings;
    }
    let lower = html.to_ascii_lowercase();
    let open_script = lower.matches("<script").count();
    let close_script = lower.matches("</script>").count();
    if open_script > close_script {
        warnings.push(format!(
            "{} unclosed <script> tag(s)",
            open_script - close_script
        ));
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> RenderRequest {
        RenderRequest {
            content_type: HtmlRenderer::content_type_id(),
            buffer_text: text.to_string(),
        }
    }

    #[test]
    fn content_type_is_html() {
        assert_eq!(HtmlRenderer.content_type().as_str(), "html");
    }

    #[test]
    fn capabilities_advertise_live_render_and_print() {
        let caps = HtmlRenderer.capabilities();
        assert!(caps.supports_live_render);
        assert!(caps.supports_print);
        assert_eq!(caps.max_safe_document_bytes, MAX_SAFE_BYTES);
    }

    #[test]
    fn document_without_styles_gets_fallback() {
        let out = HtmlRenderer
            .render(req("<html><head></head><body><p>hi</p></body></html>"))
            .unwrap();
        assert!(out.used_fallback_stylesheet);
        assert!(out.document_html.contains(FALLBACK_STYLESHEET_URL));
    }

    #[test]
    fn fallback_is_injected_after_head() {
        let out = HtmlRenderer
            .render(req("<html><head><title>t</title></head><body></body></html>"))
            .unwrap();
        let head_idx = out.document_html.find("<head>").unwrap();
        let link_idx = out.document_html.find(FALLBACK_STYLESHEET_URL).unwrap();
        let title_idx = out.document_html.find("<title>").unwrap();
        // Link sits between <head> and the rest of the head content.
        assert!(head_idx < link_idx);
        assert!(link_idx < title_idx);
    }

    #[test]
    fn document_with_inline_style_keeps_author_styles() {
        let html = "<html><head><style>body{color:red}</style></head><body></body></html>";
        let out = HtmlRenderer.render(req(html)).unwrap();
        assert!(!out.used_fallback_stylesheet);
        assert!(!out.document_html.contains(FALLBACK_STYLESHEET_URL));
        assert_eq!(out.document_html, html);
    }

    #[test]
    fn document_with_link_stylesheet_keeps_author_styles() {
        let html = "<link rel=\"stylesheet\" href=\"site.css\"><p>x</p>";
        let out = HtmlRenderer.render(req(html)).unwrap();
        assert!(!out.used_fallback_stylesheet);
    }

    #[test]
    fn link_stylesheet_detection_is_case_insensitive_and_quote_agnostic() {
        assert!(has_author_styles("<LINK REL=stylesheet HREF=a.css>"));
        assert!(has_author_styles("<link rel='stylesheet' href='a.css'>"));
        // A <link> that is not a stylesheet does not count.
        assert!(!has_author_styles("<link rel=\"icon\" href=\"f.ico\">"));
    }

    #[test]
    fn fallback_prepended_when_no_head_or_html() {
        let out = HtmlRenderer.render(req("<p>bare fragment</p>")).unwrap();
        assert!(out.used_fallback_stylesheet);
        assert!(out.document_html.starts_with("<link"));
    }

    #[test]
    fn empty_document_renders_with_fallback_and_no_warnings() {
        let out = HtmlRenderer.render(req("")).unwrap();
        assert!(out.used_fallback_stylesheet);
        assert!(out.parser_warnings.is_empty());
    }

    #[test]
    fn oversized_document_is_refused() {
        let big = "a".repeat((MAX_SAFE_BYTES + 1) as usize);
        let err = HtmlRenderer.render(req(&big)).unwrap_err();
        assert!(matches!(err, RenderError::DocumentTooLarge { .. }));
    }

    #[test]
    fn unclosed_script_is_flagged() {
        let out = HtmlRenderer
            .render(req("<body><script>doStuff()</body>"))
            .unwrap();
        assert_eq!(out.parser_warnings.len(), 1);
        assert!(out.parser_warnings[0].contains("unclosed"));
    }
}
