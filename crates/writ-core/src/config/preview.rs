//! `[preview]` configuration section.
//!
//! Lean scope (see ADR-010/011 supersede notes): the preview renders the
//! user's own offline agent output. Per-content-type default layouts for
//! HTML and Markdown, the size thresholds and debounce that govern live
//! re-render, and the single app-level scripts kill switch. PDF / image /
//! SVG-file renderers and the detached window are cut, so their config keys
//! are gone. Every field has a serde default so existing configs upgrade
//! cleanly.

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

fn default_layout_html() -> DefaultLayout {
    DefaultLayout::Split
}

fn default_layout_markdown() -> DefaultLayout {
    DefaultLayout::Split
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

fn default_run_scripts() -> bool {
    true
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
    /// App-level scripts kill switch. When `true` (default) the document CSP
    /// permits inline + same-origin + `writ-preview:` scripts so interactive
    /// agent output (sliders, Mermaid, KaTeX) runs; network stays off
    /// regardless. When `false`, `script-src` is `'none'`. This is the only
    /// knob over the otherwise-fixed document policy — there is no per-buffer
    /// trust state.
    #[serde(default = "default_run_scripts")]
    pub run_scripts: bool,
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
            run_scripts: default_run_scripts(),
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
        assert_eq!(c.default_layout_markdown, DefaultLayout::Split);
        assert_eq!(c.live_render_threshold_mb, 1);
        assert_eq!(c.render_confirm_threshold_mb, 5);
        assert_eq!(c.render_refuse_threshold_mb, 50);
        assert_eq!(c.debounce_ms, 200);
        // Scripts on by default — interactive agent output works out of the box.
        assert!(c.run_scripts);
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
        assert_eq!(c.default_layout_markdown, DefaultLayout::Split);
        assert!(c.run_scripts);
    }

    #[test]
    fn run_scripts_kill_switch_round_trips() {
        let c = PreviewConfig {
            run_scripts: false,
            ..PreviewConfig::default()
        };
        let s = toml::to_string(&c).unwrap();
        let back: PreviewConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
        assert!(!back.run_scripts);
    }
}
