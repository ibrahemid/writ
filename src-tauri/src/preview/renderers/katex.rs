//! KaTeX math runtime — shared injection + bundled asset table, ADR-009
//! roster, L6.
//!
//! Math renders client-side from a bundled, offline KaTeX runtime served
//! same-origin under the `writ-preview://document/_assets/katex/` route (see
//! [`super::mermaid`] for why same-origin, not the chrome origin).
//!
//! The Markdown renderer tokenizes math with pulldown-cmark's `ENABLE_MATH`, so
//! each `$…$` / `$$…$$` becomes a `<span class="math math-inline|math-display">`
//! carrying the *raw* LaTeX (one node, backslashes intact, code-aware). When
//! any are present this module's runtime is injected, and a small inline pass
//! calls `katex.render` on each span. This is more robust than client-side
//! delimiter scanning: a multi-line `$$` block survives, and a lone currency
//! `$` is never mistaken for math (the parser already excluded it).
//!
//! KaTeX is a pure string→DOM renderer: no eval, no workers, no network. Its
//! output carries inline `style` attributes (covered by the document CSP's
//! `style-src 'unsafe-inline'`) and its fonts load from
//! `_assets/katex/fonts/*.woff2` (covered by `font-src writ-preview:`); the
//! relative `url(fonts/…)` in `katex.min.css` resolves against the stylesheet's
//! own URL, so no CSP change is needed.
//!
//! Degradation is automatic: scripts off → `script-src 'none'` → the runtime
//! never executes and each span shows its raw LaTeX source text.

/// Base of the same-origin asset route for the KaTeX bundle.
const BASE: &str = "writ-preview://document/_assets/katex";

/// `<head>` injection: the KaTeX stylesheet. Put in `<head>` so the first
/// paint is already styled; auto-render itself does not depend on it.
pub fn head_tags() -> String {
    format!("<link rel=\"stylesheet\" href=\"{BASE}/katex.min.css\">")
}

/// End-of-`<body>` injection: the runtime, then a pass that typesets every
/// math span the Markdown renderer emitted. Each `span.math` carries raw LaTeX
/// as its text content; `displayMode` follows the `math-display` class.
/// `throwOnError:false` keeps a malformed expression from throwing (it renders
/// the error inline instead); the `try` is belt-and-suspenders.
pub fn runtime_tags() -> String {
    format!(
        "<script src=\"{BASE}/katex.min.js\"></script>\n\
         <script>(function(){{\
         var ns=document.querySelectorAll('span.math');\
         for(var i=0;i<ns.length;i++){{var el=ns[i];\
         try{{window.katex.render(el.textContent,el,\
         {{displayMode:el.classList.contains('math-display'),throwOnError:false}});}}\
         catch(e){{}}}}\
         }})();</script>"
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
        _ => return None,
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_loads_katex_then_renders_each_math_span() {
        let tags = runtime_tags();
        // The runtime is loaded before the render pass that consumes it.
        let load = tags.find("katex.min.js").unwrap();
        let pass = tags.find("katex.render(").unwrap();
        assert!(load < pass);
        // The pass targets the spans the Markdown renderer emits and follows
        // the display/inline distinction from the class.
        assert!(tags.contains("span.math"));
        assert!(tags.contains("math-display"));
        assert!(tags.contains("throwOnError:false"));
        // No client-side delimiter scanning (auto-render) is used.
        assert!(!tags.contains("auto-render"));
        assert!(!tags.contains("renderMathInElement"));
    }

    #[test]
    fn head_tag_links_the_stylesheet() {
        assert!(head_tags().contains("katex.min.css"));
        assert!(head_tags().contains("rel=\"stylesheet\""));
    }

    #[test]
    fn asset_table_serves_js_css_and_all_twenty_fonts() {
        assert!(asset("katex.min.js").is_some());
        assert!(asset("katex.min.css").is_some());
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
        assert!(!src.contains("new Function("), "katex must not use new Function()");
        for (idx, _) in src.match_indices("Function(") {
            let prev = src[..idx].chars().next_back().unwrap_or(' ');
            assert!(
                prev == '.' || prev == '_' || prev == '$' || prev.is_ascii_alphanumeric(),
                "katex has a bare Function() constructor at byte {idx}",
            );
        }
    }
}
