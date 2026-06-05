//! Markdown renderer — `ContentRenderer` for `markdown` (ADR-009 roster, L4).
//!
//! Agents emit Markdown constantly, so this is core, not optional. The
//! renderer parses CommonMark + the GitHub-flavored extensions agents use
//! (tables, strikethrough, task lists, footnotes) with `pulldown-cmark` and
//! serializes to an HTML fragment.
//!
//! **Embedded raw HTML passes through verbatim — no sanitization.** The CSP
//! is the uniform boundary (see `docs/security/html-preview.md`): the
//! rendered HTML loads in the same fixed-CSP `writ-preview://document`
//! iframe as the HTML renderer's output, so a `<script>` or `<div>` in the
//! Markdown is governed by exactly the same policy as one in a `.html`
//! buffer. Stripping it here would be both redundant and inconsistent with
//! how `.html` is treated. `pulldown-cmark`'s default behavior is
//! passthrough, which is what we want.
//!
//! Markdown output is a fragment with no styling of its own, so the host
//! fallback stylesheet always applies (unconditionally, unlike the HTML
//! renderer's presence-conditional check).

use pulldown_cmark::{html, Options, Parser};
use writ_core::preview::{
    ContentRenderer, ContentTypeId, RenderError, RenderOutput, RenderRequest,
    RendererCapabilities,
};

use super::theme;

/// Hard ceiling, mirroring the HTML renderer and ADR-009's 50 MB refusal.
const MAX_SAFE_BYTES: u64 = 50 * 1024 * 1024;

/// The Markdown content renderer.
pub struct MarkdownRenderer;

impl MarkdownRenderer {
    /// Content-type id this renderer registers under.
    pub fn content_type_id() -> ContentTypeId {
        ContentTypeId::new("markdown")
    }

    /// The GitHub-flavored extensions agent output relies on. Raw-HTML
    /// passthrough is on by default and is deliberately not disabled.
    fn options() -> Options {
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES
    }
}

impl ContentRenderer for MarkdownRenderer {
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

        let parser = Parser::new_ext(&request.buffer_text, Self::options());
        let mut body = String::with_capacity(request.buffer_text.len() * 3 / 2);
        html::push_html(&mut body, parser);

        // Markdown carries no styling of its own, so the base theme always
        // applies. Wrap in a complete document with the theme inlined as a
        // <style> (not a cross-scope <link>, which can fail to apply).
        let document_html = theme::wrap_document(&body);

        Ok(RenderOutput {
            document_html,
            used_fallback_stylesheet: true,
            parser_warnings: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> RenderRequest {
        RenderRequest {
            content_type: MarkdownRenderer::content_type_id(),
            buffer_text: text.to_string(),
        }
    }

    fn render(text: &str) -> RenderOutput {
        MarkdownRenderer.render(req(text)).unwrap()
    }

    #[test]
    fn content_type_is_markdown() {
        assert_eq!(MarkdownRenderer.content_type().as_str(), "markdown");
    }

    #[test]
    fn capabilities_advertise_live_render_and_print() {
        let caps = MarkdownRenderer.capabilities();
        assert!(caps.supports_live_render);
        assert!(caps.supports_print);
        assert_eq!(caps.max_safe_document_bytes, MAX_SAFE_BYTES);
    }

    #[test]
    fn renders_heading_and_paragraph() {
        let out = render("# Title\n\nA paragraph.");
        assert!(out.document_html.contains("<h1>Title</h1>"));
        assert!(out.document_html.contains("<p>A paragraph.</p>"));
    }

    #[test]
    fn always_wraps_in_a_themed_document() {
        let out = render("# x");
        assert!(out.used_fallback_stylesheet);
        // Self-contained document with the theme inlined as <style>.
        assert!(out.document_html.contains("<!doctype html>"));
        assert!(out.document_html.contains("<style>"));
        assert!(out.document_html.contains("--writ-preview-bg"));
        // The inlined style precedes the rendered body.
        let style_idx = out.document_html.find("<style>").unwrap();
        let body_idx = out.document_html.find("<h1>").unwrap();
        assert!(style_idx < body_idx);
    }

    #[test]
    fn renders_gfm_table() {
        let md = "| a | b |\n| - | - |\n| 1 | 2 |";
        let out = render(md);
        assert!(out.document_html.contains("<table>"));
        assert!(out.document_html.contains("<th>a</th>"));
        assert!(out.document_html.contains("<td>1</td>"));
    }

    #[test]
    fn renders_strikethrough_and_task_lists() {
        let out = render("~~gone~~\n\n- [x] done\n- [ ] todo");
        assert!(out.document_html.contains("<del>gone</del>"));
        assert!(out.document_html.contains("type=\"checkbox\""));
    }

    #[test]
    fn renders_fenced_code_block() {
        let out = render("```rust\nfn main() {}\n```");
        assert!(out.document_html.contains("<pre><code"));
        assert!(out.document_html.contains("fn main()"));
    }

    #[test]
    fn embedded_raw_html_passes_through_unsanitized() {
        // The CSP is the boundary — the renderer must NOT strip embedded
        // HTML. A block-level raw HTML element survives verbatim.
        let out = render("Before\n\n<div class=\"callout\">raw <b>html</b></div>\n\nAfter");
        assert!(out.document_html.contains("<div class=\"callout\">raw <b>html</b></div>"));
    }

    #[test]
    fn embedded_script_passes_through_unsanitized() {
        // A <script> in Markdown is treated exactly as one in a .html
        // buffer: passed through, governed by the document CSP in the
        // iframe (script-src is host-controlled, network is off). The
        // renderer does not sanitize it away.
        let out = render("text\n\n<script>doStuff()</script>\n\nmore");
        assert!(out.document_html.contains("<script>doStuff()</script>"));
    }

    #[test]
    fn inline_raw_html_passes_through() {
        let out = render("a paragraph with <span style=\"color:red\">inline</span> html");
        assert!(out.document_html.contains("<span style=\"color:red\">inline</span>"));
    }

    #[test]
    fn oversized_document_is_refused() {
        let big = "a".repeat((MAX_SAFE_BYTES + 1) as usize);
        let err = MarkdownRenderer.render(req(&big)).unwrap_err();
        assert!(matches!(err, RenderError::DocumentTooLarge { .. }));
    }

    #[test]
    fn empty_document_still_produces_a_themed_document() {
        let out = render("");
        assert!(out.used_fallback_stylesheet);
        assert!(out.document_html.contains("<!doctype html>"));
        assert!(out.document_html.contains("--writ-preview-bg"));
    }
}
