//! Pure markdown-to-HTML-fragment core. No Tauri, no app protocol URLs.
//! The app (`src-tauri`) and the marketing site both call this so the site
//! demo renders byte-identical markup to the shipped app.

#[cfg(feature = "wasm")]
mod wasm;

use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

/// Maps a file reference written in a document to a URL the host can serve.
///
/// Returning `None` leaves the reference exactly as authored. The crate stays
/// dependency-free and knows nothing about the app protocol: `src-tauri`
/// supplies the closure, and the wasm entry point supplies none, so the site
/// renders references as plain content.
pub type AssetResolver<'a> = &'a dyn Fn(&str) -> Option<String>;

/// File extensions treated as an embeddable image in the `![[…]]` form.
const IMAGE_EXTENSIONS: [&str; 11] = [
    "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif", "ico", "tif", "tiff",
];

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
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
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

/// A note's leading YAML frontmatter block and the body after it.
pub struct Frontmatter<'a> {
    /// The block including both `---` fences and the trailing newline, or
    /// `None` when the text has none.
    pub raw: Option<&'a str>,
    /// Everything after the block; the whole text when there is none.
    pub body: &'a str,
}

/// Splits a leading YAML frontmatter block off `text` by byte offset.
///
/// Semantics match `writ_core::prompt::strip::strip_frontmatter`: the first
/// line must be exactly `---` after trimming line-end whitespace, and a later
/// line must be exactly `---`. An unterminated or malformed block is body
/// text, never swallowed. The block is never parsed, re-serialised or
/// normalised, so a round trip is byte-identical including key order,
/// quoting, comments and trailing whitespace.
pub fn split_frontmatter(text: &str) -> Frontmatter<'_> {
    let none = Frontmatter {
        raw: None,
        body: text,
    };
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return none;
    };
    if first.trim_end() != "---" {
        return none;
    }
    let mut offset = first.len();
    for line in lines {
        let end = offset + line.len();
        if line.trim_end() == "---" {
            return Frontmatter {
                raw: Some(&text[..end]),
                body: &text[end..],
            };
        }
        offset = end;
    }
    none
}

/// True when a recognised frontmatter block holds no content lines.
///
/// pulldown-cmark 0.13.4 needs at least one content line to open a YAML
/// metadata block, so `---\n---\n` reaches the parser as two thematic breaks
/// and never produces the `MetadataBlock` events the render loop drops.
fn block_is_blank(raw: &str) -> bool {
    let mut lines = raw.split_inclusive('\n').skip(1).peekable();
    while let Some(line) = lines.next() {
        if lines.peek().is_some() && !line.trim().is_empty() {
            return false;
        }
    }
    true
}

/// Parse markdown with Writ's exact options, rewriting mermaid fences and
/// passing math through, and serialize to an HTML fragment.
///
/// A leading YAML frontmatter block is hidden: the parser reports it as a
/// `MetadataBlock` the event loop drops, and a blank block, which the parser
/// does not recognise, is split off first.
pub fn render_markdown_fragment(text: &str) -> MarkdownFragment {
    render_markdown_fragment_with(text, None)
}

/// [`render_markdown_fragment`] with an optional file resolver.
///
/// When a resolver is supplied, three reference forms are rewritten to the
/// URL it returns: the Markdown image `![](img.png)`, the Obsidian embed
/// `![[img.png]]`, and a `src` attribute on a raw `<img>` tag. A reference
/// the resolver declines is left as authored.
///
/// Without a resolver the output is byte-identical to what the crate has
/// always produced, which is what keeps the site demo and the app in step on
/// every document that embeds nothing.
pub fn render_markdown_fragment_with(
    text: &str,
    asset_url: Option<AssetResolver<'_>>,
) -> MarkdownFragment {
    let embedded;
    let text = match asset_url {
        Some(_) => {
            embedded = rewrite_image_embeds(text);
            embedded.as_str()
        }
        None => text,
    };
    let source = match split_frontmatter(text) {
        Frontmatter {
            raw: Some(raw),
            body,
        } if block_is_blank(raw) => body,
        _ => text,
    };
    let parser = Parser::new_ext(source, options());
    let mut events: Vec<Event> = Vec::new();
    let mut has_mermaid = false;
    let mut has_math = false;
    let mut in_mermaid = false;
    let mut in_metadata = false;
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
            Event::Start(Tag::MetadataBlock(_)) => in_metadata = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_metadata = false,
            _ if in_metadata => {}
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                has_math = true;
                events.push(event);
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = match asset_url.and_then(|resolve| resolve(&dest_url)) {
                    Some(url) => CowStr::from(url),
                    None => dest_url,
                };
                events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            Event::Html(raw) => events.push(Event::Html(rewrite_html_img_src(raw, asset_url))),
            Event::InlineHtml(raw) => {
                events.push(Event::InlineHtml(rewrite_html_img_src(raw, asset_url)))
            }
            other => events.push(other),
        }
    }
    let mut html_out = String::with_capacity(source.len() * 3 / 2);
    html::push_html(&mut html_out, events.into_iter());
    MarkdownFragment {
        html: html_out,
        has_mermaid,
        has_math,
    }
}

/// Rewrite the Obsidian image embed `![[img.png]]` into the Markdown image
/// `![img.png](<img.png>)`, so both reference forms reach the resolver
/// through one code path.
///
/// Only references that name an image file are rewritten; `![[Some Note]]`
/// is a note embed and stays as authored. Fenced and indented code blocks
/// and inline code spans are left alone.
fn rewrite_image_embeds(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<(char, usize)> = None;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim_start();
        match fence {
            Some((marker, len)) => {
                if closes_fence(trimmed, marker, len) {
                    fence = None;
                }
                out.push_str(line);
            }
            None if body.starts_with("    ") || body.starts_with('\t') => out.push_str(line),
            None => match opens_fence(trimmed) {
                Some(open) => {
                    fence = Some(open);
                    out.push_str(line);
                }
                None => rewrite_embeds_in_line(line, &mut out),
            },
        }
    }
    out
}

/// The fence marker and run length a line opens a fenced code block with.
fn opens_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    (run >= 3).then_some((marker, run))
}

/// True when a line closes the open fence: the same marker, at least as long,
/// and nothing else on the line.
fn closes_fence(trimmed: &str, marker: char, len: usize) -> bool {
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    run >= len && trimmed[run..].trim().is_empty()
}

fn rewrite_embeds_in_line(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut code_span: Option<usize> = None;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let run = i - start;
            code_span = match code_span {
                Some(open) if open == run => None,
                open @ Some(_) => open,
                None => Some(run),
            };
            out.push_str(&line[start..i]);
            continue;
        }
        if code_span.is_none() && line[i..].starts_with("![[") {
            if let Some(end) = line[i + 3..].find("]]") {
                if let Some(image) = image_embed(&line[i + 3..i + 3 + end]) {
                    out.push_str(&image);
                    i += 3 + end + 2;
                    continue;
                }
            }
        }
        let ch = line[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
}

/// The Markdown image an embed body maps to, or `None` when the body does
/// not name an image file.
///
/// An Obsidian display option after `|` (a width, usually) is dropped: the
/// file name is the alt text, which is the accessible reading of an embed
/// that carries no caption.
fn image_embed(body: &str) -> Option<String> {
    let target = body.split('|').next()?.trim();
    if target.is_empty() || target.contains(['[', ']', '<', '>', '(', ')']) {
        return None;
    }
    let extension = target.rsplit_once('.')?.1.to_ascii_lowercase();
    if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let name = target.rsplit(['/', '\\']).next().unwrap_or(target);
    Some(format!("![{name}](<{target}>)"))
}

/// Rewrite the `src` of every raw `<img>` tag in a chunk of embedded HTML.
fn rewrite_html_img_src<'a>(raw: CowStr<'a>, asset_url: Option<AssetResolver<'_>>) -> CowStr<'a> {
    let Some(resolve) = asset_url else {
        return raw;
    };
    let lower = raw.to_ascii_lowercase();
    if !lower.contains("<img") {
        return raw;
    }
    let mut out = String::with_capacity(raw.len());
    let mut cursor = 0;
    let mut rewrote = false;
    while let Some(offset) = lower[cursor..].find("<img") {
        let name_end = cursor + offset + "<img".len();
        if !lower[name_end..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            out.push_str(&raw[cursor..name_end]);
            cursor = name_end;
            continue;
        }
        let Some(tag_end) = lower[name_end..].find('>').map(|i| name_end + i) else {
            break;
        };
        out.push_str(&raw[cursor..name_end]);
        match rewrite_src_attribute(&raw[name_end..tag_end], &lower[name_end..tag_end], resolve) {
            Some(rewritten) => {
                out.push_str(&rewritten);
                rewrote = true;
            }
            None => out.push_str(&raw[name_end..tag_end]),
        }
        cursor = tag_end;
    }
    if !rewrote {
        return raw;
    }
    out.push_str(&raw[cursor..]);
    CowStr::from(out)
}

/// Replace the value of the `src` attribute inside one tag body.
///
/// `lower` is the ASCII-lowercased twin of `body`; `to_ascii_lowercase`
/// preserves byte length, so offsets found in one index the other.
fn rewrite_src_attribute(body: &str, lower: &str, resolve: AssetResolver<'_>) -> Option<String> {
    let mut search = 0;
    loop {
        let name_at = lower[search..].find("src")? + search;
        search = name_at + "src".len();
        let starts_attribute =
            name_at == 0 || lower[..name_at].ends_with(|c: char| c.is_whitespace());
        let after_name = &lower[search..];
        let after_space = after_name.trim_start();
        if !starts_attribute || !after_space.starts_with('=') {
            continue;
        }
        let value_at = search + (after_name.len() - after_space.len()) + 1;
        let area = &body[value_at..];
        let padding = area.len() - area.trim_start().len();
        let area = &area[padding..];
        let (value, quote) = match area.chars().next() {
            Some(quote @ ('"' | '\'')) => (&area[1..1 + area[1..].find(quote)?], 1),
            _ => (
                &area[..area.find(char::is_whitespace).unwrap_or(area.len())],
                0,
            ),
        };
        let url = resolve(value)?;
        let value_at = value_at + padding + quote;
        let mut out = String::with_capacity(body.len() + url.len());
        out.push_str(&body[..value_at]);
        out.push_str(&url);
        out.push_str(&body[value_at + value.len()..]);
        return Some(out);
    }
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

    /// Resolver standing in for the host: names an asset URL for anything
    /// that looks like a file beside the note, declines everything else.
    fn served(reference: &str) -> Option<String> {
        if reference.starts_with("http") || reference.starts_with("data:") {
            return None;
        }
        Some(format!(
            "writ-preview://document/_note-asset/b/n/{reference}"
        ))
    }

    fn with_assets(text: &str) -> String {
        render_markdown_fragment_with(text, Some(&served)).html
    }

    #[test]
    fn markdown_image_reference_is_rewritten() {
        let html = with_assets("![shot](attachments/a.png)");
        assert!(html.contains("src=\"writ-preview://document/_note-asset/b/n/attachments/a.png\""));
        assert!(html.contains("alt=\"shot\""));
    }

    #[test]
    fn obsidian_embed_becomes_an_image_named_by_its_file() {
        let html = with_assets("before\n\n![[attachments/a.png]]\n\nafter");
        assert!(html.contains("src=\"writ-preview://document/_note-asset/b/n/attachments/a.png\""));
        assert!(html.contains("alt=\"a.png\""));
    }

    #[test]
    fn obsidian_embed_drops_a_display_option_and_keeps_spaces() {
        let html = with_assets("![[my shot.png|300]]");
        // The serializer percent-encodes the space it is handed and leaves an
        // already-encoded `%` alone, so a resolver that returns an encoded URL
        // survives the round trip unchanged.
        assert!(html.contains("src=\"writ-preview://document/_note-asset/b/n/my%20shot.png\""));
        assert!(html.contains("alt=\"my shot.png\""));
    }

    #[test]
    fn raw_img_tag_src_is_rewritten() {
        let html = with_assets("<img src=\"a.png\" width=\"20\">");
        assert!(html.contains("src=\"writ-preview://document/_note-asset/b/n/a.png\""));
        assert!(html.contains("width=\"20\""));

        let inline = with_assets("text <img src='b.png' alt='x'/> more");
        assert!(inline.contains("src='writ-preview://document/_note-asset/b/n/b.png'"));
        assert!(inline.contains("alt='x'"));
    }

    #[test]
    fn a_reference_the_resolver_declines_is_left_as_authored() {
        let html = with_assets("![x](https://example.com/a.png)\n\n<img src=\"data:image/png,x\">");
        assert!(html.contains("src=\"https://example.com/a.png\""));
        assert!(html.contains("src=\"data:image/png,x\""));
    }

    #[test]
    fn without_a_resolver_every_form_is_untouched() {
        let text = "![shot](a.png)\n\n![[a.png]]\n\n<img src=\"a.png\">";
        let html = render_markdown_fragment(text).html;
        assert!(html.contains("src=\"a.png\""));
        assert!(html.contains("![[a.png]]"));
        assert!(!html.contains("_note-asset"));
    }

    #[test]
    fn an_embed_inside_code_stays_literal() {
        let fenced = with_assets("```\n![[a.png]]\n```");
        assert!(fenced.contains("![[a.png]]"));
        assert!(!fenced.contains("_note-asset"));

        let inline = with_assets("use `![[a.png]]` for an image");
        assert!(inline.contains("![[a.png]]"));
        assert!(!inline.contains("_note-asset"));

        let indented = with_assets("    ![[a.png]]\n");
        assert!(indented.contains("![[a.png]]"));
        assert!(!indented.contains("_note-asset"));
    }

    #[test]
    fn a_note_embed_is_not_treated_as_an_image() {
        let html = with_assets("![[Some Note]]");
        assert!(html.contains("![[Some Note]]"));
        assert!(!html.contains("<img"));
    }

    #[test]
    fn a_fence_reopens_rewriting_after_it_closes() {
        let html = with_assets("```rust\n![[a.png]]\n```\n\n![[b.png]]");
        assert!(html.contains("![[a.png]]"));
        assert!(html.contains("_note-asset/b/n/b.png"));
    }

    #[test]
    fn a_tag_that_merely_starts_with_img_is_not_rewritten() {
        let html = with_assets("<imgx src=\"a.png\"></imgx>");
        assert!(html.contains("src=\"a.png\""));
        assert!(!html.contains("_note-asset"));
    }
}
