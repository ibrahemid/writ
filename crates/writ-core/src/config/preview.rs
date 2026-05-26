//! `[preview]` configuration section — ADR-009 §"Configuration".
//!
//! Per-content-type default layouts plus the size thresholds and debounce
//! that govern live re-render. Every field has a serde default so existing
//! configs upgrade cleanly.

use serde::{Deserialize, Serialize};

/// Default layout a content type opens in.
///
/// A subset of `LayoutMode`: the configurable surface only exposes the
/// non-parameterized choices. `Split` resolves to the 50/50 vertical split
/// at open time; `Detached` is never a *default* (it is opt-in per buffer).
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

fn default_layout_html() -> DefaultLayout {
    DefaultLayout::Split
}

fn default_layout_markdown() -> DefaultLayout {
    DefaultLayout::Split
}

fn default_layout_pdf() -> DefaultLayout {
    DefaultLayout::Preview
}

fn default_layout_image() -> DefaultLayout {
    DefaultLayout::Preview
}

fn default_layout_svg() -> DefaultLayout {
    DefaultLayout::Preview
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

fn default_detach_on_open() -> bool {
    false
}

/// Preview surface configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewConfig {
    /// Default layout for HTML documents.
    #[serde(default = "default_layout_html")]
    pub default_layout_html: DefaultLayout,
    /// Default layout for Markdown documents.
    #[serde(default = "default_layout_markdown")]
    pub default_layout_markdown: DefaultLayout,
    /// Default layout for PDF documents.
    #[serde(default = "default_layout_pdf")]
    pub default_layout_pdf: DefaultLayout,
    /// Default layout for raster images.
    #[serde(default = "default_layout_image")]
    pub default_layout_image: DefaultLayout,
    /// Default layout for SVG documents.
    #[serde(default = "default_layout_svg")]
    pub default_layout_svg: DefaultLayout,
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
    /// Whether opening a renderable buffer immediately detaches its preview
    /// to a second window.
    #[serde(default = "default_detach_on_open")]
    pub detach_on_open: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            default_layout_html: default_layout_html(),
            default_layout_markdown: default_layout_markdown(),
            default_layout_pdf: default_layout_pdf(),
            default_layout_image: default_layout_image(),
            default_layout_svg: default_layout_svg(),
            live_render_threshold_mb: default_live_render_threshold_mb(),
            render_confirm_threshold_mb: default_render_confirm_threshold_mb(),
            render_refuse_threshold_mb: default_render_refuse_threshold_mb(),
            debounce_ms: default_debounce_ms(),
            detach_on_open: default_detach_on_open(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_adr_009() {
        let c = PreviewConfig::default();
        assert_eq!(c.default_layout_html, DefaultLayout::Split);
        assert_eq!(c.default_layout_markdown, DefaultLayout::Split);
        assert_eq!(c.default_layout_pdf, DefaultLayout::Preview);
        assert_eq!(c.default_layout_image, DefaultLayout::Preview);
        assert_eq!(c.default_layout_svg, DefaultLayout::Preview);
        assert_eq!(c.live_render_threshold_mb, 1);
        assert_eq!(c.render_confirm_threshold_mb, 5);
        assert_eq!(c.render_refuse_threshold_mb, 50);
        assert_eq!(c.debounce_ms, 200);
        assert!(!c.detach_on_open);
    }

    #[test]
    fn empty_table_yields_defaults() {
        let c: PreviewConfig = toml::from_str("").unwrap();
        assert_eq!(c, PreviewConfig::default());
    }

    #[test]
    fn partial_table_keeps_other_defaults() {
        let c: PreviewConfig = toml::from_str("default_layout_html = \"source\"\ndebounce_ms = 50").unwrap();
        assert_eq!(c.default_layout_html, DefaultLayout::Source);
        assert_eq!(c.debounce_ms, 50);
        // Untouched fields keep their defaults.
        assert_eq!(c.default_layout_markdown, DefaultLayout::Split);
        assert_eq!(c.render_refuse_threshold_mb, 50);
    }

    #[test]
    fn round_trips_through_toml() {
        let c = PreviewConfig {
            default_layout_html: DefaultLayout::Preview,
            detach_on_open: true,
            ..PreviewConfig::default()
        };
        let s = toml::to_string(&c).unwrap();
        let back: PreviewConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
    }
}
