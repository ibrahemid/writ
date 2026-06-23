//! Pure markdown-to-HTML-fragment core. No Tauri, no app protocol URLs.
//! The app (`src-tauri`) and the marketing site both call this so the site
//! demo renders byte-identical markup to the shipped app.

#[cfg(feature = "wasm")]
mod wasm;

use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// HTML fragment plus the runtime-injection flags the caller needs.
pub struct MarkdownFragment {
    pub html: String,
    pub has_mermaid: bool,
    pub has_math: bool,
}

/// Writ's GFM + math option set. Raw-HTML passthrough stays on by default.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
}

/// True when a fenced-code info string selects the Mermaid renderer: the first
/// whitespace-delimited token equals `mermaid`, case-insensitive. (Verbatim
/// from `mermaid.rs:51-55` — case-insensitivity is load-bearing for parity.)
pub fn is_mermaid_info(info: &str) -> bool {
    info.split_whitespace()
        .next()
        .is_some_and(|tok| tok.eq_ignore_ascii_case("mermaid"))
}

/// Minimal HTML text escaping for `<pre>` content: `&`, `<`, `>` only.
/// (Verbatim from `mermaid.rs:65-76`.)
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

/// The `<pre class="mermaid">` block the bundled Mermaid runtime renders from.
/// Escapes the source as-is — does NOT trim (the caller passes the raw fence
/// body; the empty-fence guard lives in `render_markdown_fragment`). Verbatim
/// from `mermaid.rs:60-62`.
pub fn diagram_block(source: &str) -> String {
    format!("<pre class=\"mermaid\">{}</pre>", escape_text(source))
}

/// Parse markdown with Writ's exact options, rewriting mermaid fences and
/// passing math through, and serialize to an HTML fragment.
pub fn render_markdown_fragment(text: &str) -> MarkdownFragment {
    let parser = Parser::new_ext(text, options());
    let mut events: Vec<Event> = Vec::new();
    let mut has_mermaid = false;
    let mut has_math = false;
    let mut in_mermaid = false;
    let mut mermaid_src = String::new();
    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info)))
                if is_mermaid_info(info) =>
            {
                in_mermaid = true;
                mermaid_src.clear();
            }
            Event::Text(t) if in_mermaid => mermaid_src.push_str(&t),
            Event::End(TagEnd::CodeBlock) if in_mermaid => {
                in_mermaid = false;
                if !mermaid_src.trim().is_empty() {
                    has_mermaid = true;
                    events.push(Event::Html(diagram_block(&mermaid_src).into()));
                }
            }
            _ if in_mermaid => {}
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                has_math = true;
                events.push(event);
            }
            other => events.push(other),
        }
    }
    let mut html_out = String::with_capacity(text.len() * 3 / 2);
    html::push_html(&mut html_out, events.into_iter());
    MarkdownFragment { html: html_out, has_mermaid, has_math }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_heading_and_paragraph() {
        let f = render_markdown_fragment("# Title\n\nbody");
        assert!(f.html.contains("<h1>Title</h1>"));
        assert!(f.html.contains("<p>body</p>"));
        assert!(!f.has_mermaid && !f.has_math);
    }

    #[test]
    fn renders_gfm_table_strikethrough_tasklist() {
        let f = render_markdown_fragment("| a |\n|---|\n| 1 |\n\n~~x~~\n\n- [ ] todo");
        assert!(f.html.contains("<table>"));
        assert!(f.html.contains("<del>x</del>"));
        assert!(f.html.contains("type=\"checkbox\""));
    }

    #[test]
    fn mermaid_fence_becomes_pre_and_flags_mermaid() {
        let f = render_markdown_fragment("```mermaid\nA-->B\n```");
        assert!(f.html.contains("class=\"mermaid\""));
        assert!(f.has_mermaid);
    }

    #[test]
    fn non_mermaid_fence_stays_code_and_no_flag() {
        let f = render_markdown_fragment("```rust\nfn main() {}\n```");
        assert!(f.html.contains("fn main()"));
        assert!(!f.has_mermaid);
    }

    #[test]
    fn empty_mermaid_fence_emits_nothing_and_no_flag() {
        let f = render_markdown_fragment("```mermaid\n\n```");
        assert!(!f.has_mermaid);
        assert!(!f.html.contains("class=\"mermaid\""));
    }

    #[test]
    fn multiline_block_math_kept_intact_and_flags_math() {
        let f = render_markdown_fragment("$$\n\\int_0^1 x\\,dx\n$$");
        assert!(f.has_math);
        assert!(f.html.contains("\\int_0^1 x\\,dx"));
    }

    #[test]
    fn inline_math_flags_math() {
        let f = render_markdown_fragment("value $x^2$ here");
        assert!(f.has_math);
    }
}
