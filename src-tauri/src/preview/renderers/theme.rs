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
//!
//! That CSS is two files: the `--writ-preview-*` token layer generated from
//! `design/tokens` into `assets/generated/preview-tokens.css`, and the rules
//! in `assets/preview-base.css` that read it. They are inlined in that order,
//! and this module is the only thing that knows they are separate.

use writ_core::preview::ThemePolarity;

/// The bundled fallback stylesheet's rules, compiled into the binary.
pub const PREVIEW_BASE_CSS: &str = include_str!("../../../assets/preview-base.css");

/// The generated `--writ-preview-*` token layer the rules read, compiled into
/// the binary. Light on `:root`, dark under `[data-writ-theme="dark"]`.
pub const PREVIEW_TOKENS_CSS: &str = include_str!("../../../assets/generated/preview-tokens.css");

/// The base theme as an inline `<style>` element: tokens first, then the rules
/// that read them.
pub fn style_tag() -> String {
    format!("<style>{PREVIEW_TOKENS_CSS}\n{PREVIEW_BASE_CSS}</style>")
}

/// The base theme for a document that keeps its own `<html>` tag (an HTML
/// buffer), where the polarity cannot be carried as an attribute on the root.
/// The dark palette is re-declared on `:root` after the token layer so it wins
/// without touching the author's markup; light is the layer's default.
pub fn style_tag_for(polarity: ThemePolarity) -> String {
    match polarity {
        ThemePolarity::Light => style_tag(),
        ThemePolarity::Dark => match dark_palette_block() {
            Some(block) => {
                format!("<style>{PREVIEW_TOKENS_CSS}\n:root{block}\n{PREVIEW_BASE_CSS}</style>")
            }
            None => style_tag(),
        },
    }
}

/// The `{ ... }` body of the `[data-writ-theme="dark"]` rule in the token layer.
fn dark_palette_block() -> Option<&'static str> {
    let start = PREVIEW_TOKENS_CSS.find("[data-writ-theme=\"dark\"] {")?;
    let rest = &PREVIEW_TOKENS_CSS[start..];
    let open = rest.find('{')?;
    let close = rest[open..].find('}')? + open;
    Some(&rest[open..=close])
}

/// Same-origin URL of the bundled first-party preview bridge runtime.
pub const BRIDGE_URL: &str = "writ-preview://document/_assets/preview/bridge.js";

/// The first-party bridge as a `<script>` element loaded same-origin.
///
/// The bridge wires scroll-sync and in-preview find between the app shell and
/// the cross-origin preview iframe over `postMessage`. It is injected into
/// every rendered document (loaded under the document CSP's
/// `script-src 'self' writ-preview:`); when the scripts kill switch is off it
/// is blocked along with all other scripts and the features degrade silently.
pub fn bridge_script_tag() -> String {
    format!("<script src=\"{BRIDGE_URL}\"></script>")
}

/// Editor zoom within this tolerance of native size is treated as "no zoom",
/// so a default-zoom render stays byte-identical to the pre-zoom output.
const ZOOM_EPSILON: f64 = 1e-4;

/// A CSS `zoom` value for the document root, or `None` when the factor is
/// effectively native (1.0) or invalid. Clamped to a sane range and trimmed
/// of trailing zeros so `2.0` serializes as `2`, not `2.0000`.
pub fn css_zoom(zoom: f64) -> Option<String> {
    if !zoom.is_finite() || zoom <= 0.0 {
        return None;
    }
    let clamped = zoom.clamp(0.1, 10.0);
    if (clamped - 1.0).abs() < ZOOM_EPSILON {
        return None;
    }
    let formatted = format!("{clamped:.4}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    Some(trimmed.to_string())
}

/// Wrap an HTML body fragment (e.g. Markdown output) in a complete,
/// self-contained document with the inlined base theme and a UTF-8 charset.
pub fn wrap_document(body_fragment: &str) -> String {
    wrap_document_with("", body_fragment, "", ThemePolarity::Light, 1.0)
}

/// Like [`wrap_document`], but injects extra `<head>` content (e.g. a runtime
/// stylesheet `<link>`) and extra content immediately before `</body>` (e.g. a
/// runtime `<script>`). Renderers that enhance the document with a bundled
/// runtime (Mermaid in L5, KaTeX in L6) use this to load those runtimes from
/// the same-origin `writ-preview://document/_assets/` route.
pub fn wrap_document_with(
    head_extra: &str,
    body_fragment: &str,
    body_end_extra: &str,
    polarity: ThemePolarity,
    zoom: f64,
) -> String {
    let mut attrs = String::new();
    if let ThemePolarity::Dark = polarity {
        attrs.push_str(" data-writ-theme=\"dark\"");
    }
    if let Some(z) = css_zoom(zoom) {
        attrs.push_str(&format!(" style=\"zoom:{z}\""));
    }
    let html_open = format!("<html{attrs}>");
    format!(
        "<!doctype html>\n{html_open}\n<head>\n<meta charset=\"utf-8\">\n{style}\n{head_extra}\n</head>\n<body>\n{body_fragment}\n{body_end_extra}\n{bridge}\n</body>\n</html>",
        style = style_tag(),
        bridge = bridge_script_tag(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The `--writ-preview-*` palette 0.3.5 served, captured verbatim from
    /// origin/main's `preview-base.css` before the token layer moved into
    /// `design/tokens`. The served palette is compared against it so a change
    /// to the token sources cannot silently repaint the reading surface.
    const ORIGIN_PALETTE: &str =
        include_str!("../../../tests/fixtures/preview-palette-origin-main.css");

    fn strip_comments(css: &str) -> String {
        let mut out = String::new();
        let mut rest = css;
        while let Some(start) = rest.find("/*") {
            out.push_str(&rest[..start]);
            match rest[start + 2..].find("*/") {
                Some(end) => rest = &rest[start + 2 + end + 2..],
                None => return out,
            }
        }
        out.push_str(rest);
        out
    }

    /// The `--writ-preview-*` declarations that survive the cascade when
    /// `selectors` all match the document root. `:root` and
    /// `[data-writ-theme="…"]` carry the same specificity, so source order
    /// settles a name both declare.
    fn palette(css: &str, selectors: &[&str]) -> BTreeMap<String, String> {
        let stripped = strip_comments(css);
        let mut resolved = BTreeMap::new();
        let mut rest: &str = &stripped;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}').map(|at| open + at) else {
                break;
            };
            // Everything after the previous rule's close brace is this rule's
            // selector; an at-rule's own brace leaves a stray `}` behind.
            let selector = rest[..open].rsplit('}').next().unwrap_or_default().trim();
            if selectors.contains(&selector) {
                for declaration in rest[open + 1..close].split(';') {
                    let Some((name, value)) = declaration.split_once(':') else {
                        continue;
                    };
                    let name = name.trim();
                    if name.starts_with("--writ-preview-") {
                        resolved.insert(name.to_string(), value.trim().to_string());
                    }
                }
            }
            rest = &rest[close + 1..];
        }
        resolved
    }

    fn style_block(document: &str) -> String {
        let open = document.find("<style>").expect("a style tag") + "<style>".len();
        let close = document.find("</style>").expect("a closed style tag");
        document[open..close].to_string()
    }

    /// The palette a served document actually paints: its `:root` layer plus
    /// whichever `data-writ-theme` layer its `<html>` element opts into.
    fn served_palette(document: &str) -> BTreeMap<String, String> {
        // Read the attribute off the <html> open tag only: the inlined sheet
        // names the same attribute in its own selectors.
        let open_tag_at = document.find("<html").expect("an html element");
        let open_tag_end = document[open_tag_at..]
            .find('>')
            .expect("a closed html open tag")
            + open_tag_at;
        let open_tag = &document[open_tag_at..open_tag_end];
        let mut selectors = vec![":root".to_string()];
        if let Some(at) = open_tag.find("data-writ-theme=\"") {
            let rest = &open_tag[at + "data-writ-theme=\"".len()..];
            let end = rest.find('"').expect("a closed attribute value");
            selectors.push(format!("[data-writ-theme=\"{}\"]", &rest[..end]));
        }
        let refs: Vec<&str> = selectors.iter().map(String::as_str).collect();
        palette(&style_block(document), &refs)
    }

    #[test]
    fn a_dark_app_serves_the_palette_0_3_5_served() {
        assert_eq!(
            served_palette(&wrap_document_with(
                "",
                "<p>x</p>",
                "",
                ThemePolarity::Dark,
                1.0
            )),
            palette(ORIGIN_PALETTE, &[":root"]),
        );
    }

    #[test]
    fn a_light_app_serves_the_light_palette_0_3_5_served() {
        assert_eq!(
            served_palette(&wrap_document_with(
                "",
                "<p>x</p>",
                "",
                ThemePolarity::Light,
                1.0
            )),
            palette(ORIGIN_PALETTE, &[":root", "[data-writ-theme=\"light\"]"]),
        );
    }

    #[test]
    fn an_html_buffer_resolves_the_same_palette_from_root_alone() {
        // An HTML buffer keeps its own <html>, so the polarity cannot ride an
        // attribute; the sheet re-declares the wanted layer on :root instead.
        for (polarity, expected) in [
            (ThemePolarity::Dark, palette(ORIGIN_PALETTE, &[":root"])),
            (
                ThemePolarity::Light,
                palette(ORIGIN_PALETTE, &[":root", "[data-writ-theme=\"light\"]"]),
            ),
        ] {
            let css = style_block(&style_tag_for(polarity));
            assert_eq!(palette(&css, &[":root"]), expected, "{polarity:?}");
        }
    }

    #[test]
    fn dark_palette_block_is_found() {
        let block = dark_palette_block().expect("the token layer declares a dark block");
        assert!(block.starts_with('{') && block.ends_with('}'));
        assert!(block.contains("--writ-preview-bg: #0e0e14"));
        assert!(
            !block.contains("#fbfbfd"),
            "the block must not be the light :root layer"
        );
    }

    #[test]
    fn dark_style_tag_redeclares_the_dark_palette_on_root() {
        let tag = style_tag_for(ThemePolarity::Dark);
        let at = tag.rfind(":root{").expect("a :root override is appended");
        assert!(tag[at..].contains("--writ-preview-bg: #0e0e14"));
        assert!(at > tag.find("[data-writ-theme=\"dark\"] {").unwrap());
    }

    #[test]
    fn light_style_tag_is_the_plain_base_sheet() {
        assert_eq!(style_tag_for(ThemePolarity::Light), style_tag());
    }

    #[test]
    fn base_css_is_nonempty_and_rules_only() {
        // Sanity: the compiled stylesheet reads the token layer and no longer
        // declares it, so an empty or wrong file fails the build-time include.
        assert!(PREVIEW_BASE_CSS.contains("var(--writ-preview-bg)"));
        assert!(!PREVIEW_BASE_CSS.contains("--writ-preview-bg:"));
        assert!(PREVIEW_BASE_CSS.len() > 1000);
    }

    #[test]
    fn style_tag_inlines_tokens_before_rules() {
        let tag = style_tag();
        assert!(tag.starts_with("<style>"));
        assert!(tag.ends_with("</style>"));
        let tokens = tag
            .find("--writ-preview-bg:")
            .expect("the token layer is inlined");
        let rules = tag
            .find("var(--writ-preview-bg)")
            .expect("the rules are inlined");
        assert!(
            tokens < rules,
            "tokens must be declared before they are read"
        );
    }

    #[test]
    fn wrap_document_injects_the_preview_bridge_script_before_body_close() {
        // Every wrapped document (Markdown, standalone Mermaid) carries the
        // first-party bridge so scroll-sync / in-preview find work. It is a
        // same-origin _assets script, injected at the body tail.
        let doc = wrap_document("<p>x</p>");
        assert!(doc.contains("writ-preview://document/_assets/preview/bridge.js"));
        let script_idx = doc.find("/preview/bridge.js").unwrap();
        let body_close = doc.find("</body>").unwrap();
        assert!(
            script_idx < body_close,
            "bridge script must sit inside <body>"
        );
    }

    #[test]
    fn dark_document_carries_the_dark_attribute() {
        let dark = wrap_document_with("", "<p>x</p>", "", ThemePolarity::Dark, 1.0);
        assert!(
            dark.contains("<html data-writ-theme=\"dark\">"),
            "dark render must carry the data-writ-theme attribute so the served \
             document opens in the dark palette without a flash"
        );
    }

    #[test]
    fn light_document_carries_no_theme_attribute() {
        // Light is the token layer's `:root` default, so a light document opts
        // into nothing. (The inlined layer legitimately names the
        // `[data-writ-theme="dark"]` selector, so assert on the open tag form.)
        let light = wrap_document_with("", "<p>x</p>", "", ThemePolarity::Light, 1.0);
        assert!(light.contains("\n<html>\n"));
        assert!(!light.contains("data-writ-theme=\"dark\">"));
    }

    #[test]
    fn non_native_zoom_is_baked_onto_the_document_root() {
        // A re-render of a zoomed preview must open already-scaled, so the
        // factor rides on the <html> root as an inline zoom (no 1x pop).
        let doc = wrap_document_with("", "<p>x</p>", "", ThemePolarity::Light, 2.0);
        assert!(doc.contains("<html style=\"zoom:2\">"), "got: {doc}");

        // It composes with the dark palette on the same root element.
        let dark = wrap_document_with("", "<p>x</p>", "", ThemePolarity::Dark, 1.5);
        assert!(
            dark.contains("<html data-writ-theme=\"dark\" style=\"zoom:1.5\">"),
            "got: {dark}"
        );
    }

    #[test]
    fn native_zoom_leaves_the_root_byte_stable() {
        // Default zoom must not perturb the served bytes (dedup + existing
        // snapshots depend on it), and invalid factors fall back to native.
        let native = wrap_document_with("", "<p>x</p>", "", ThemePolarity::Light, 1.0);
        assert!(native.contains("\n<html>\n"));
        assert!(!native.contains("style=\"zoom"));
        assert_eq!(css_zoom(1.0), None);
        assert_eq!(css_zoom(f64::NAN), None);
        assert_eq!(css_zoom(0.0), None);
        assert_eq!(css_zoom(-2.0), None);
        assert_eq!(css_zoom(2.0).as_deref(), Some("2"));
        assert_eq!(css_zoom(0.5).as_deref(), Some("0.5"));
        // Clamped to the sane ceiling.
        assert_eq!(css_zoom(1000.0).as_deref(), Some("10"));
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
