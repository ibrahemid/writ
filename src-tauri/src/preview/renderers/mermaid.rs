//! Mermaid renderer + shared runtime injection — ADR-009 roster, L5.
//!
//! Mermaid diagrams render client-side from a bundled, offline runtime
//! (`mermaid.min.js`, the v11.15.0 single-file IIFE) served **same-origin**
//! under the `_assets/` route, as is the first-party init script that
//! configures and runs it. Both are loaded from the document origin (a
//! cross-origin `writ-preview://chrome` subresource is refused by that scope's
//! `Cross-Origin-Resource-Policy: same-origin`), with root-relative URLs so
//! they resolve on Windows too, and both are covered by the document CSP's
//! `'self'`.
//!
//! The Markdown renderer rewrites ```mermaid fences into `<pre class="mermaid">`
//! blocks and injects the runtime when at least one is present; this module
//! owns both the standalone `.mmd` renderer and the injection snippet they
//! share.
//!
//! The runtime and its init are first-party, not the document's own scripts,
//! so they run under `script-src 'self'` whether or not the file is opted in
//! to run its scripts (decision RC-D1/TS-D1): a diagram in the user's own note
//! draws either way. The init being an external same-origin file, not inline,
//! is what lets it run when author inline scripts are blocked.

use writ_core::preview::{
    ContentRenderer, ContentTypeId, RenderError, RenderOutput, RenderRequest, RendererCapabilities,
};

use super::theme;

/// Hard ceiling, mirroring the other renderers and ADR-009's refusal size.
const MAX_SAFE_BYTES: u64 = 50 * 1024 * 1024;

/// Root-relative URL of the bundled Mermaid runtime, served by the `_assets`
/// document route (see `handler::chrome_asset`). Relative, not absolute
/// `writ-preview://…`, so it resolves against the document's own origin on
/// Windows as well (TS-03/DS-03).
pub const RUNTIME_URL: &str = "/_assets/mermaid/mermaid.min.js";

/// Root-relative URL of the first-party init script that configures and runs
/// Mermaid. It is an external same-origin script, not inline, so it executes
/// under `script-src 'self'` when the document is not opted in to run its own
/// scripts — diagrams draw whether or not author scripts are allowed.
pub const INIT_URL: &str = "/_assets/mermaid/init.js";

/// The `<script>` tags that load the runtime and the first-party init.
///
/// `src-tauri/assets/mermaid/init.js` configures Mermaid with
/// `securityLevel: 'strict'` (sanitises diagram-authored HTML, and unlike
/// `'sandbox'` does not wrap each diagram in an iframe — forbidden by the
/// document CSP's `frame-src 'none'`) and a theme that follows the document's
/// polarity, then runs every `<pre class="mermaid">`.
pub fn runtime_tags() -> String {
    format!(
        "<script src=\"{RUNTIME_URL}\"></script>\n\
         <script src=\"{INIT_URL}\"></script>"
    )
}

pub use writ_render::{diagram_block, escape_text, is_mermaid_info};

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
        let document_html =
            theme::wrap_document_with("", &body, &runtime_tags(), request.theme, request.zoom);

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
                theme: Default::default(),
                zoom: 1.0,
                assets: None,
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
        assert!(out
            .document_html
            .contains("<pre class=\"mermaid\">graph TD; A--&gt;B</pre>"));
        assert!(out.document_html.contains(RUNTIME_URL));
        assert!(out.document_html.contains(INIT_URL));
        assert!(out.used_fallback_stylesheet);
    }

    /// The init script the runtime tags load, as the asset table serves it.
    const INIT_JS: &str = include_str!("../../../assets/mermaid/init.js");

    #[test]
    fn runtime_tags_load_the_runtime_then_the_external_init() {
        let tags = runtime_tags();
        // Both are external same-origin scripts (so they run under
        // script-src 'self' when the file is not opted in), the runtime first.
        assert!(tags.contains(&format!("<script src=\"{RUNTIME_URL}\">")));
        assert!(tags.contains(&format!("<script src=\"{INIT_URL}\">")));
        assert!(tags.find(RUNTIME_URL).unwrap() < tags.find(INIT_URL).unwrap());
        // No inline init, so nothing here needs 'unsafe-inline'.
        assert!(!tags.contains("mermaid.initialize"));
    }

    #[test]
    fn the_init_asset_picks_the_theme_and_runs_strict() {
        assert!(INIT_JS.contains("data-writ-theme"));
        assert!(INIT_JS.contains("\"dark\""));
        assert!(INIT_JS.contains("securityLevel: \"strict\""));
        assert!(INIT_JS.contains("mermaid.run("));
    }

    #[test]
    fn oversized_document_is_refused() {
        let big = "a".repeat((MAX_SAFE_BYTES + 1) as usize);
        let err = MermaidRenderer
            .render(RenderRequest {
                content_type: MermaidRenderer::content_type_id(),
                buffer_text: big,
                theme: Default::default(),
                zoom: 1.0,
                assets: None,
            })
            .unwrap_err();
        assert!(matches!(err, RenderError::DocumentTooLarge { .. }));
    }
}
