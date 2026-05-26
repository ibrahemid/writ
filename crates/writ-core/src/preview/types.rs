//! Pure-domain preview surface types — ADR-009.

use serde::{Deserialize, Serialize};

/// Stable identifier for a content type understood by the renderer registry.
///
/// Identifiers are lowercased, dotted, ASCII strings — for example `html`,
/// `markdown`, `image.svg`, `image.png`. The convention is "kind" with an
/// optional "subkind" appended after a dot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentTypeId(String);

impl ContentTypeId {
    /// Construct a [`ContentTypeId`] from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ContentTypeId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ContentTypeId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for ContentTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Monotonic identifier for an OS-level Writ window.
///
/// The main window is `WindowId(1)`. Detached preview windows (Phase 2) get
/// the next monotonic id assigned by `src-tauri`'s window manager. The id is
/// a logical Writ-side handle; it is not the OS window handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(pub u64);

impl WindowId {
    /// The id reserved for Writ's main (always-present) window.
    pub const MAIN: WindowId = WindowId(1);
}

/// Which view a tab is showing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    /// CodeMirror source view (the editor).
    Source,
    /// Rendered preview pane.
    Preview,
}

/// How a tab arranges its source and preview surfaces.
///
/// Per ADR-009 §B3: `Source` shows the editor only; `Preview` shows the
/// rendered preview only; `Split` shows both side by side with a stored
/// ratio; `Detached` indicates the preview is hosted in a different
/// [`WindowId`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutMode {
    /// Source-only view.
    Source,
    /// Preview-only view.
    Preview,
    /// Side-by-side split between source and preview.
    Split {
        /// Fractional width of the source pane (0.0..=1.0).
        ratio: f32,
        /// Whether the split divider runs vertically (left/right) or
        /// horizontally (top/bottom).
        orientation: SplitOrientation,
    },
    /// Source-only here; preview is mounted in another window.
    Detached {
        /// The [`WindowId`] that hosts the detached preview.
        window_id: WindowId,
    },
}

/// Direction of a split divider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitOrientation {
    /// Divider runs top-to-bottom; source on the left, preview on the right.
    Vertical,
    /// Divider runs left-to-right; source on top, preview on the bottom.
    Horizontal,
}

impl LayoutMode {
    /// The default layout for content types that combine authoring with
    /// preview (HTML, markdown).
    pub fn default_split() -> Self {
        Self::Split {
            ratio: 0.5,
            orientation: SplitOrientation::Vertical,
        }
    }
}

/// Capabilities advertised by a [`ContentRenderer`].
///
/// `max_safe_document_bytes` is the renderer's own upper bound. The surface-
/// level thresholds in ADR-009 §C (1 MB, 5 MB, 50 MB) are policy and apply
/// in addition to whatever each renderer reports here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererCapabilities {
    /// Whether keystroke-debounced re-render is supported.
    pub supports_live_render: bool,
    /// Whether the renderer can produce printable output.
    pub supports_print: bool,
    /// Renderer-specific size ceiling above which the renderer refuses to
    /// render at all.
    pub max_safe_document_bytes: u64,
}

/// Inputs to a single render invocation.
///
/// Lean scope: no per-document trust policy and no workspace root. The
/// preview renders the user's own offline agent output under one fixed
/// CSP (the only variable is the app-level scripts kill switch, applied
/// at serve time, not per render). See the ADR-011/010 supersede notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderRequest {
    /// Content type the renderer was selected for.
    pub content_type: ContentTypeId,
    /// Live buffer text. Renderers operate on in-memory bytes, not on
    /// on-disk content.
    pub buffer_text: String,
}

/// Result of a successful render invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderOutput {
    /// HTML to inject into the preview document.
    pub document_html: String,
    /// Whether the host's fallback stylesheet should be served alongside
    /// (per ADR-009 F2 presence-conditional rule).
    pub used_fallback_stylesheet: bool,
    /// Parser warnings collected during render (surfaced in the chrome
    /// affordance per ADR-009 failure modes).
    pub parser_warnings: Vec<String>,
}

/// Errors a renderer may surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderError {
    /// Input exceeds the renderer's max safe document size.
    DocumentTooLarge {
        /// Size of the input document.
        bytes: u64,
        /// Renderer's declared limit.
        limit: u64,
    },
    /// Input is malformed in a way the renderer refuses to handle.
    InvalidInput {
        /// Free-text reason surfaced to the UI.
        reason: String,
    },
    /// Renderer-internal failure.
    Internal {
        /// Free-text reason surfaced to the UI.
        reason: String,
    },
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DocumentTooLarge { bytes, limit } => {
                write!(f, "document too large: {bytes} bytes exceeds {limit}")
            }
            Self::InvalidInput { reason } => write!(f, "invalid input: {reason}"),
            Self::Internal { reason } => write!(f, "internal renderer error: {reason}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// The contract every preview renderer implements.
///
/// First-party renderers (HTML, markdown, Mermaid, math, PDF, SVG, raster
/// image) live in `src-tauri/preview/renderers/` and each implement this
/// trait. Per ADR-009 §D1 the registry stores them as `Box<dyn ContentRenderer>`.
pub trait ContentRenderer: Send + Sync {
    /// The content type this renderer handles. Used as the registry key.
    fn content_type(&self) -> ContentTypeId;

    /// What the renderer is capable of for surface-level policy decisions.
    fn capabilities(&self) -> RendererCapabilities;

    /// Render a document. Pure: must not touch the filesystem or network.
    fn render(&self, request: RenderRequest) -> Result<RenderOutput, RenderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn content_type_id_round_trip() {
        let id = ContentTypeId::new("markdown");
        let s = serde_json::to_string(&id).unwrap();
        let back: ContentTypeId = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
        assert_eq!(id.as_str(), "markdown");
        assert_eq!(format!("{id}"), "markdown");
    }

    #[test]
    fn window_id_main_is_one() {
        assert_eq!(WindowId::MAIN.0, 1);
        let s = serde_json::to_string(&WindowId::MAIN).unwrap();
        assert_eq!(s, "1");
    }

    #[test]
    fn layout_mode_serde_round_trip() {
        let modes = [
            LayoutMode::Source,
            LayoutMode::Preview,
            LayoutMode::default_split(),
            LayoutMode::Split {
                ratio: 0.7,
                orientation: SplitOrientation::Horizontal,
            },
            LayoutMode::Detached {
                window_id: WindowId(42),
            },
        ];
        for mode in modes {
            let s = serde_json::to_string(&mode).unwrap();
            let back: LayoutMode = serde_json::from_str(&s).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn view_mode_serde_round_trip() {
        for mode in [ViewMode::Source, ViewMode::Preview] {
            let s = serde_json::to_string(&mode).unwrap();
            let back: ViewMode = serde_json::from_str(&s).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn render_error_display() {
        let err = RenderError::DocumentTooLarge {
            bytes: 50 * 1024 * 1024 + 1,
            limit: 50 * 1024 * 1024,
        };
        assert!(format!("{err}").contains("too large"));
    }
}
