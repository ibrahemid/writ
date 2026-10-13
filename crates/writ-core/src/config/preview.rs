//! `[preview]` configuration section.
//!
//! Per-content-type default layouts for HTML and Markdown, and the size
//! thresholds and debounce that govern live re-render. Whether a document's
//! own scripts run is not here: it is opted in per file and kept on the
//! buffer row (ADR-043). PDF / image / SVG-file renderers and the detached
//! window are cut, so their config keys are gone. Every field has a serde
//! default so existing configs upgrade cleanly.

use serde::{Deserialize, Serialize};

/// Default layout a content type opens in.
///
/// A subset of `LayoutMode`: the configurable surface exposes only the
/// non-parameterized choices. `Split` resolves to the 50/50 vertical split
/// at open time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultLayout {
    /// Source editor only.
    Source,
    /// Side-by-side split.
    Split,
    /// Preview only.
    Preview,
}

/// Default layout a Markdown document opens in.
///
/// Separate from `DefaultLayout` so the retired `split` and `preview` values
/// alias here only: `serde(alias)` is per variant, and HTML still offers both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkdownLayout {
    /// Rendered in the editor.
    #[serde(alias = "split", alias = "preview")]
    Inline,
    /// Raw Markdown source.
    Source,
}

fn default_layout_html() -> DefaultLayout {
    DefaultLayout::Split
}

fn default_layout_markdown() -> MarkdownLayout {
    MarkdownLayout::Inline
}

fn default_live_render_threshold_mb() -> u32 {
    1
}

fn default_render_confirm_threshold_mb() -> u32 {
    5
}

fn default_render_refuse_threshold_mb() -> u32 {
    50
}

fn default_debounce_ms() -> u32 {
    200
}

/// Preview surface configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewConfig {
    /// Default layout for HTML documents.
    #[serde(default = "default_layout_html")]
    pub default_layout_html: DefaultLayout,
    /// Default layout for Markdown documents.
    #[serde(default = "default_layout_markdown")]
    pub default_layout_markdown: MarkdownLayout,
    /// Above this document size (MB) live re-render auto-disables and the
    /// surface offers manual refresh (Cmd+R).
    #[serde(default = "default_live_render_threshold_mb")]
    pub live_render_threshold_mb: u32,
    /// Above this document size (MB) the surface asks before rendering.
    #[serde(default = "default_render_confirm_threshold_mb")]
    pub render_confirm_threshold_mb: u32,
    /// Above this document size (MB) the surface refuses to render and
    /// forces source view.
    #[serde(default = "default_render_refuse_threshold_mb")]
    pub render_refuse_threshold_mb: u32,
    /// Debounce, in milliseconds, between the last keystroke and a live
    /// re-render.
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u32,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            default_layout_html: default_layout_html(),
            default_layout_markdown: default_layout_markdown(),
            live_render_threshold_mb: default_live_render_threshold_mb(),
            render_confirm_threshold_mb: default_render_confirm_threshold_mb(),
            render_refuse_threshold_mb: default_render_refuse_threshold_mb(),
            debounce_ms: default_debounce_ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_lean() {
        let c = PreviewConfig::default();
        assert_eq!(c.default_layout_html, DefaultLayout::Split);
        assert_eq!(c.default_layout_markdown, MarkdownLayout::Inline);
        assert_eq!(c.live_render_threshold_mb, 1);
        assert_eq!(c.render_confirm_threshold_mb, 5);
        assert_eq!(c.render_refuse_threshold_mb, 50);
        assert_eq!(c.debounce_ms, 200);
    }

    #[test]
    fn empty_table_yields_defaults() {
        let c: PreviewConfig = toml::from_str("").unwrap();
        assert_eq!(c, PreviewConfig::default());
    }

    #[test]
    fn partial_table_keeps_other_defaults() {
        let c: PreviewConfig =
            toml::from_str("default_layout_html = \"source\"\ndebounce_ms = 50").unwrap();
        assert_eq!(c.default_layout_html, DefaultLayout::Source);
        assert_eq!(c.debounce_ms, 50);
        assert_eq!(c.default_layout_markdown, MarkdownLayout::Inline);
    }

    #[test]
    fn markdown_layout_aliases_the_old_values() {
        let split: PreviewConfig = toml::from_str("default_layout_markdown = \"split\"").unwrap();
        assert_eq!(split.default_layout_markdown, MarkdownLayout::Inline);
        let preview: PreviewConfig =
            toml::from_str("default_layout_markdown = \"preview\"").unwrap();
        assert_eq!(preview.default_layout_markdown, MarkdownLayout::Inline);
        let source: PreviewConfig = toml::from_str("default_layout_markdown = \"source\"").unwrap();
        assert_eq!(source.default_layout_markdown, MarkdownLayout::Source);
    }

    // A config written by an older build carried `run_scripts` (now retired,
    // scripts are off for every document with a per-file opt-in). The key is
    // ignored rather than refused so those configs still load.
    #[test]
    fn a_retired_run_scripts_key_is_ignored() {
        let c: PreviewConfig = toml::from_str("run_scripts = true\ndebounce_ms = 50").unwrap();
        assert_eq!(c.debounce_ms, 50);
        assert_eq!(
            c,
            PreviewConfig {
                debounce_ms: 50,
                ..PreviewConfig::default()
            }
        );
    }
}
