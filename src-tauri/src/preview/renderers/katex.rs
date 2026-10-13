//! KaTeX math runtime — shared injection + bundled asset table, ADR-009
//! roster, L6.
//!
//! Math renders client-side from a bundled, offline KaTeX runtime served
//! same-origin under the `_assets/katex/` route (see [`super::mermaid`] for
//! why same-origin, not the chrome origin), with a root-relative URL so it
//! resolves on Windows too.
//!
//! The Markdown renderer tokenizes math with pulldown-cmark's `ENABLE_MATH`, so
//! each `$…$` / `$$…$$` becomes a `<span class="math math-inline|math-display">`
//! carrying the *raw* LaTeX (one node, backslashes intact, code-aware). When
//! any are present this module's runtime is injected, and a first-party init
//! script (`_assets/katex/init.js`) calls `katex.render` on each span. This is
//! more robust than client-side delimiter scanning: a multi-line `$$` block
//! survives, and a lone currency `$` is never mistaken for math.
//!
//! KaTeX is a pure string→DOM renderer: no eval, no workers, no network. Its
//! output carries inline `style` attributes (covered by the document CSP's
//! `style-src 'unsafe-inline'`) and its fonts load from
//! `_assets/katex/fonts/*.woff2` (covered by `font-src 'self'`); the relative
//! `url(fonts/…)` in `katex.min.css` resolves against the stylesheet's own URL.
//!
//! The runtime and its init are first-party, not the document's own scripts,
//! so they run under `script-src 'self'` whether or not the file is opted in to
//! run its scripts (decision RC-D1/TS-D1): math in the user's own note draws
//! either way. The init being an external same-origin file, not inline, is what
//! lets it run when author inline scripts are blocked.

/// Root-relative base of the same-origin asset route for the KaTeX bundle.
const BASE: &str = "/_assets/katex";

/// `<head>` injection: the KaTeX stylesheet. Put in `<head>` so the first
/// paint is already styled; auto-render itself does not depend on it.
pub fn head_tags() -> String {
    format!("<link rel=\"stylesheet\" href=\"{BASE}/katex.min.css\">")
}

/// End-of-`<body>` injection: the runtime, then the first-party init that
/// typesets every math span the Markdown renderer emitted. Both are external
/// same-origin scripts (not inline), so they run under `script-src 'self'`
/// when the document is not opted in to run its own scripts.
pub fn runtime_tags() -> String {
    format!(
        "<script src=\"{BASE}/katex.min.js\"></script>\n\
         <script src=\"{BASE}/init.js\"></script>"
    )
}

/// The bundled KaTeX asset bytes for a path under `katex/` (js, css, and the
/// woff2 fonts). The 20 fonts are woff2-only: `katex.min.css` lists woff2 first
/// in every `src`, so the absent woff/ttf fallbacks are never requested.
pub fn asset(path: &str) -> Option<&'static [u8]> {
    let bytes: &'static [u8] = match path {
        "katex.min.js" => include_bytes!("../../../assets/katex/katex.min.js"),
        "katex.min.css" => include_bytes!("../../../assets/katex/katex.min.css"),
        "fonts/KaTeX_AMS-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_AMS-Regular.woff2")
        }
        "fonts/KaTeX_Caligraphic-Bold.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Caligraphic-Bold.woff2")
        }
        "fonts/KaTeX_Caligraphic-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Caligraphic-Regular.woff2")
        }
        "fonts/KaTeX_Fraktur-Bold.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Fraktur-Bold.woff2")
        }
        "fonts/KaTeX_Fraktur-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Fraktur-Regular.woff2")
        }
        "fonts/KaTeX_Main-Bold.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Main-Bold.woff2")
        }
        "fonts/KaTeX_Main-BoldItalic.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Main-BoldItalic.woff2")
        }
        "fonts/KaTeX_Main-Italic.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Main-Italic.woff2")
        }
        "fonts/KaTeX_Main-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Main-Regular.woff2")
        }
        "fonts/KaTeX_Math-BoldItalic.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Math-BoldItalic.woff2")
        }
        "fonts/KaTeX_Math-Italic.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Math-Italic.woff2")
        }
        "fonts/KaTeX_SansSerif-Bold.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_SansSerif-Bold.woff2")
        }
        "fonts/KaTeX_SansSerif-Italic.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_SansSerif-Italic.woff2")
        }
        "fonts/KaTeX_SansSerif-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_SansSerif-Regular.woff2")
        }
        "fonts/KaTeX_Script-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Script-Regular.woff2")
        }
        "fonts/KaTeX_Size1-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Size1-Regular.woff2")
        }
        "fonts/KaTeX_Size2-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Size2-Regular.woff2")
        }
        "fonts/KaTeX_Size3-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Size3-Regular.woff2")
        }
        "fonts/KaTeX_Size4-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Size4-Regular.woff2")
        }
        "fonts/KaTeX_Typewriter-Regular.woff2" => {
            include_bytes!("../../../assets/katex/fonts/KaTeX_Typewriter-Regular.woff2")
        }
        // The first-party init that typesets the math spans, served same-origin
        // so it runs under `script-src 'self'` whether or not scripts are on.
        "init.js" => include_bytes!("../../../assets/katex/init.js"),
        _ => return None,
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The init script the runtime tags load, as the asset table serves it.
    const INIT_JS: &str = include_str!("../../../assets/katex/init.js");

    #[test]
    fn runtime_tags_load_the_runtime_then_the_external_init() {
        let tags = runtime_tags();
        // Both are external same-origin scripts (so they run under
        // script-src 'self' when the file is not opted in), the runtime first.
        let load = tags.find("katex.min.js").unwrap();
        let init = tags.find("/_assets/katex/init.js").unwrap();
        assert!(load < init);
        // No inline render pass, so nothing here needs 'unsafe-inline'.
        assert!(!tags.contains("katex.render("));
    }

    #[test]
    fn the_init_asset_typesets_each_math_span() {
        // The pass targets the spans the Markdown renderer emits and follows
        // the display/inline distinction from the class.
        assert!(INIT_JS.contains("katex.render("));
        assert!(INIT_JS.contains("span.math"));
        assert!(INIT_JS.contains("math-display"));
        assert!(INIT_JS.contains("throwOnError: false"));
        // No client-side delimiter scanning (auto-render) is used.
        assert!(!INIT_JS.contains("auto-render"));
        assert!(!INIT_JS.contains("renderMathInElement"));
    }

    #[test]
    fn head_tag_links_the_stylesheet() {
        assert!(head_tags().contains("katex.min.css"));
        assert!(head_tags().contains("rel=\"stylesheet\""));
    }

    #[test]
    fn asset_table_serves_js_css_init_and_all_twenty_fonts() {
        assert!(asset("katex.min.js").is_some());
        assert!(asset("katex.min.css").is_some());
        assert!(asset("init.js").is_some());
        // Auto-render is no longer bundled (per-span katex.render is used).
        assert!(asset("auto-render.min.js").is_none());
        assert!(asset("nope.js").is_none());
        let names = [
            "KaTeX_AMS-Regular",
            "KaTeX_Caligraphic-Bold",
            "KaTeX_Caligraphic-Regular",
            "KaTeX_Fraktur-Bold",
            "KaTeX_Fraktur-Regular",
            "KaTeX_Main-Bold",
            "KaTeX_Main-BoldItalic",
            "KaTeX_Main-Italic",
            "KaTeX_Main-Regular",
            "KaTeX_Math-BoldItalic",
            "KaTeX_Math-Italic",
            "KaTeX_SansSerif-Bold",
            "KaTeX_SansSerif-Italic",
            "KaTeX_SansSerif-Regular",
            "KaTeX_Script-Regular",
            "KaTeX_Size1-Regular",
            "KaTeX_Size2-Regular",
            "KaTeX_Size3-Regular",
            "KaTeX_Size4-Regular",
            "KaTeX_Typewriter-Regular",
        ];
        // Every font present AND distinct: the woff2 files all differ in size,
        // so a copy-paste swap (an arm pointing at the wrong include_bytes!)
        // collapses two lengths and fails here.
        let mut lengths = std::collections::HashSet::new();
        for name in names {
            let p = format!("fonts/{name}.woff2");
            let bytes = asset(&p).unwrap_or_else(|| panic!("missing font {p}"));
            assert!(lengths.insert(bytes.len()), "duplicate font bytes for {p}");
        }
        assert_eq!(lengths.len(), 20);
    }

    #[test]
    fn vendored_runtime_requires_no_unsafe_eval() {
        // The document CSP grants no 'unsafe-eval'. Guard a future re-vendor
        // from introducing eval or a `Function(<code>)` constructor. KaTeX's
        // bare `Function(` hits are all identifier-prefixed method names
        // (`callFunction(` etc.), which this scan allows; a real constructor
        // (non-identifier prefix) would fail.
        let src = std::str::from_utf8(asset("katex.min.js").unwrap()).unwrap();
        assert!(!src.contains("eval("), "katex must not use eval()");
        assert!(
            !src.contains("new Function("),
            "katex must not use new Function()"
        );
        for (idx, _) in src.match_indices("Function(") {
            let prev = src[..idx].chars().next_back().unwrap_or(' ');
            assert!(
                prev == '.' || prev == '_' || prev == '$' || prev.is_ascii_alphanumeric(),
                "katex has a bare Function() constructor at byte {idx}",
            );
        }
    }
}
