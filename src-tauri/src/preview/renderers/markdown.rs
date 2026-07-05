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

use writ_core::preview::{
    ContentRenderer, ContentTypeId, RenderError, RenderOutput, RenderRequest, RendererCapabilities,
};

use super::{katex, mermaid, theme};

/// Hard ceiling, mirroring the HTML renderer and ADR-009's 50 MB refusal.
const MAX_SAFE_BYTES: u64 = 50 * 1024 * 1024;

/// The Markdown content renderer.
pub struct MarkdownRenderer;

impl MarkdownRenderer {
    /// Content-type id this renderer registers under.
    pub fn content_type_id() -> ContentTypeId {
        ContentTypeId::new("markdown")
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
        let fragment = writ_render::render_markdown_fragment(&request.buffer_text);
        let head_extra = if fragment.has_math {
            katex::head_tags()
        } else {
            String::new()
        };
        let mut body_end = String::new();
        if fragment.has_mermaid {
            body_end.push_str(&mermaid::runtime_tags());
            body_end.push('\n');
        }
        if fragment.has_math {
            body_end.push_str(&katex::runtime_tags());
        }
        let document_html = theme::wrap_document_with(
            &head_extra,
            &fragment.html,
            &body_end,
            request.theme,
            request.zoom,
        );
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
            theme: Default::default(),
            zoom: 1.0,
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
    fn every_document_carries_the_preview_bridge() {
        // Plain Markdown (no diagram, no math) still gets the first-party
        // bridge so scroll-sync / in-preview find work on ordinary prose.
        let out = render("# title\n\njust text");
        assert!(out.document_html.contains(super::theme::BRIDGE_URL));
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
    fn mermaid_fence_becomes_mermaid_pre_and_injects_runtime() {
        let out = render("```mermaid\ngraph TD; A-->B\n```");
        // The fence is rewritten to the element Mermaid renders from, not a
        // language-tagged code block.
        assert!(out.document_html.contains("<pre class=\"mermaid\">"));
        assert!(!out.document_html.contains("language-mermaid"));
        // Diagram source is HTML-escaped so it survives as text content.
        assert!(out.document_html.contains("graph TD; A--&gt;B"));
        // The bundled runtime is injected exactly once, same-origin.
        assert!(out
            .document_html
            .contains("writ-preview://document/_assets/mermaid/mermaid.min.js"));
        assert!(out.document_html.contains("mermaid.run("));
    }

    #[test]
    fn non_mermaid_fence_stays_a_code_block_and_injects_no_runtime() {
        let out = render("```rust\nfn main() {}\n```");
        assert!(out.document_html.contains("<pre><code"));
        assert!(!out.document_html.contains("mermaid.min.js"));
    }

    #[test]
    fn plain_markdown_injects_no_mermaid_runtime() {
        let out = render("# title\n\ntext");
        assert!(!out.document_html.contains("mermaid"));
    }

    #[test]
    fn multiple_mermaid_fences_render_each_and_inject_runtime_once() {
        let md =
            "```mermaid\nA-->B\n```\n\nprose\n\n```rust\nfn x(){}\n```\n\n```mermaid\nC-->D\n```";
        let out = render(md);
        let html = &out.document_html;
        // Two diagrams, each captured (clear() between fences worked).
        assert_eq!(html.matches("<pre class=\"mermaid\">").count(), 2);
        assert!(html.contains("A--&gt;B"));
        assert!(html.contains("C--&gt;D"));
        // The interleaved rust fence stays an ordinary code block.
        assert!(html.contains("<pre><code"));
        // The runtime is injected exactly once regardless of diagram count.
        assert_eq!(html.matches("mermaid.min.js").count(), 1);
    }

    #[test]
    fn multiline_mermaid_fence_captures_full_body() {
        let out = render("```mermaid\ngraph TD\n  A-->B\n  B-->C\n```");
        assert!(out
            .document_html
            .contains("graph TD\n  A--&gt;B\n  B--&gt;C"));
    }

    #[test]
    fn empty_mermaid_fence_emits_nothing_and_no_runtime() {
        let out = render("```mermaid\n```");
        assert!(!out.document_html.contains("<pre class=\"mermaid\">"));
        assert!(!out.document_html.contains("mermaid.min.js"));
    }

    #[test]
    fn block_math_emits_display_span_and_injects_runtime() {
        let out = render("Energy:\n\n$$E = mc^2$$\n");
        // Math is tokenized into a display-math span carrying the raw content.
        assert!(out.document_html.contains("math math-display"));
        assert!(out.document_html.contains("E = mc^2"));
        // KaTeX CSS + runtime are injected.
        assert!(out
            .document_html
            .contains("writ-preview://document/_assets/katex/katex.min.css"));
        assert!(out
            .document_html
            .contains("writ-preview://document/_assets/katex/katex.min.js"));
        assert!(out.document_html.contains("katex.render("));
    }

    #[test]
    fn inline_math_emits_inline_span_and_injects_runtime() {
        let out = render("The value $x^2$ is positive.");
        assert!(out.document_html.contains("math math-inline"));
        assert!(out.document_html.contains("katex.min.js"));
    }

    #[test]
    fn multiline_block_math_keeps_latex_intact_through_the_real_pipeline() {
        // Regression (smoke L6): a $$ block on its own lines used to reach the
        // DOM as raw, backslash-mangled text — markdown did not tokenize it, so
        // the in-browser pass could not match it. With math tokenization it is a
        // single display-math span with the LaTeX preserved (`\,` not collapsed
        // to `,`, `\int` intact), which the runtime can typeset.
        let out = render("$$\n\\int_0^\\infty e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2}\n$$");
        assert!(out.document_html.contains("math math-display"));
        assert!(out.document_html.contains("\\int_0^\\infty"));
        assert!(
            out.document_html.contains("\\,dx"),
            "thin-space \\, must survive, not collapse to ,"
        );
        assert!(out.document_html.contains("\\frac{\\sqrt{\\pi}}{2}"));
        assert!(out.document_html.contains("katex.min.js"));
    }

    #[test]
    fn plain_markdown_and_lone_dollar_inject_no_katex() {
        assert!(!render("# title\n\njust text")
            .document_html
            .contains("katex"));
        // A lone currency dollar is not math and must not pull in the runtime.
        assert!(!render("it costs $5 today").document_html.contains("katex"));
    }

    #[test]
    fn document_with_both_diagram_and_math_injects_both_runtimes_in_order() {
        let out = render("```mermaid\nA-->B\n```\n\n$$x^2$$");
        let html = &out.document_html;
        assert!(html.contains("mermaid.min.js"));
        assert!(html.contains("katex.min.js"));
        // Mermaid runtime is appended before KaTeX in the body tail.
        assert!(html.find("mermaid.min.js").unwrap() < html.find("katex.min.js").unwrap());
    }

    #[test]
    fn embedded_raw_html_passes_through_unsanitized() {
        // The CSP is the boundary — the renderer must NOT strip embedded
        // HTML. A block-level raw HTML element survives verbatim.
        let out = render("Before\n\n<div class=\"callout\">raw <b>html</b></div>\n\nAfter");
        assert!(out
            .document_html
            .contains("<div class=\"callout\">raw <b>html</b></div>"));
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
        assert!(out
            .document_html
            .contains("<span style=\"color:red\">inline</span>"));
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
