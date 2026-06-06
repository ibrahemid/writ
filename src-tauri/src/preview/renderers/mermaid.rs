//! Mermaid renderer + shared runtime injection — ADR-009 roster, L5.
//!
//! Mermaid diagrams render client-side from a bundled, offline runtime
//! (`mermaid.min.js`, the v11.15.0 single-file IIFE) served **same-origin**
//! under the `writ-preview://document/_assets/` route. The runtime is loaded
//! from the document origin, not the chrome origin, because a cross-origin
//! custom-scheme subresource is refused by the chrome response's
//! `Cross-Origin-Resource-Policy: same-origin`; serving it under the document
//! origin keeps the load same-origin and needs no change to the locked
//! document CSP (covered by its existing `'self'` / `writ-preview:` sources).
//!
//! The Markdown renderer rewrites ```mermaid fences into `<pre class="mermaid">`
//! blocks and injects the runtime when at least one is present; this module
//! owns both the standalone `.mmd` renderer and the injection snippet they
//! share.
//!
//! Degradation needs no code path: when the scripts kill switch is off the
//! document CSP becomes `script-src 'none'`, the runtime never executes, and
//! each `<pre class="mermaid">` shows its raw diagram source as a code block.

use writ_core::preview::{
    ContentRenderer, ContentTypeId, RenderError, RenderOutput, RenderRequest,
    RendererCapabilities,
};

use super::theme;

/// Hard ceiling, mirroring the other renderers and ADR-009's refusal size.
const MAX_SAFE_BYTES: u64 = 50 * 1024 * 1024;

/// Same-origin URL of the bundled Mermaid runtime, served by the `_assets`
/// document route (see `handler::chrome_asset`).
pub const RUNTIME_URL: &str = "writ-preview://document/_assets/mermaid/mermaid.min.js";

/// The `<script>` block that loads the runtime and renders every
/// `<pre class="mermaid">` in the document.
///
/// `securityLevel: 'strict'` is deliberate: it sanitizes diagram-authored
/// HTML and, unlike `'sandbox'`, does not wrap each diagram in an iframe —
/// iframes are forbidden by the document CSP's `frame-src 'none'`.
pub fn runtime_tags() -> String {
    format!(
        "<script src=\"{RUNTIME_URL}\"></script>\n\
         <script>window.mermaid.initialize({{startOnLoad:false,securityLevel:'strict'}});window.mermaid.run({{querySelector:'pre.mermaid'}});</script>"
    )
}

/// True when a fenced-code info string selects the Mermaid renderer: the first
/// whitespace-delimited token equals `mermaid` (case-insensitive), matching
/// how agents tag fences (```mermaid).
pub fn is_mermaid_info(info: &str) -> bool {
    info.split_whitespace()
        .next()
        .is_some_and(|tok| tok.eq_ignore_ascii_case("mermaid"))
}

/// Wrap raw diagram source as a `<pre class="mermaid">` block, HTML-escaping
/// the source so it survives as element text. Mermaid reads `textContent`,
/// which decodes the entities back to the original diagram text.
pub fn diagram_block(source: &str) -> String {
    format!("<pre class=\"mermaid\">{}</pre>", escape_text(source))
}

/// Minimal HTML text escaping for `<pre>` content: `&`, `<`, `>`.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Standalone renderer for `.mmd` / `.mermaid` buffers: the entire buffer is a
/// single diagram.
pub struct MermaidRenderer;

impl MermaidRenderer {
    /// Content-type id this renderer registers under.
    pub fn content_type_id() -> ContentTypeId {
        ContentTypeId::new("mermaid")
    }
}

impl ContentRenderer for MermaidRenderer {
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

        let body = diagram_block(&request.buffer_text);
        let document_html = theme::wrap_document_with("", &body, &runtime_tags());

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

    fn render(text: &str) -> RenderOutput {
        MermaidRenderer
            .render(RenderRequest {
                content_type: MermaidRenderer::content_type_id(),
                buffer_text: text.to_string(),
            })
            .unwrap()
    }

    #[test]
    fn content_type_is_mermaid() {
        assert_eq!(MermaidRenderer.content_type().as_str(), "mermaid");
    }

    #[test]
    fn is_mermaid_info_matches_first_token_case_insensitively() {
        assert!(is_mermaid_info("mermaid"));
        assert!(is_mermaid_info("Mermaid"));
        assert!(is_mermaid_info("mermaid foo"));
        assert!(!is_mermaid_info("rust"));
        assert!(!is_mermaid_info("mermaidx"));
        assert!(!is_mermaid_info(""));
    }

    #[test]
    fn escape_text_escapes_markup_chars_only() {
        assert_eq!(escape_text("A --> B & <C>"), "A --&gt; B &amp; &lt;C&gt;");
        assert_eq!(escape_text("plain text 123"), "plain text 123");
    }

    #[test]
    fn diagram_block_wraps_escaped_source_in_mermaid_pre() {
        let block = diagram_block("graph TD; A-->B");
        assert_eq!(block, "<pre class=\"mermaid\">graph TD; A--&gt;B</pre>");
    }

    #[test]
    fn standalone_render_wraps_whole_buffer_and_injects_runtime() {
        let out = render("graph TD; A-->B");
        assert!(out.document_html.contains("<!doctype html>"));
        assert!(out.document_html.contains("<pre class=\"mermaid\">graph TD; A--&gt;B</pre>"));
        assert!(out.document_html.contains(RUNTIME_URL));
        assert!(out.document_html.contains("mermaid.run("));
        assert!(out.used_fallback_stylesheet);
    }

    #[test]
    fn runtime_uses_strict_security_level() {
        assert!(runtime_tags().contains("securityLevel:'strict'"));
    }

    #[test]
    fn oversized_document_is_refused() {
        let big = "a".repeat((MAX_SAFE_BYTES + 1) as usize);
        let err = MermaidRenderer
            .render(RenderRequest {
                content_type: MermaidRenderer::content_type_id(),
                buffer_text: big,
            })
            .unwrap_err();
        assert!(matches!(err, RenderError::DocumentTooLarge { .. }));
    }
}
