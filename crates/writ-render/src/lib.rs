//! Pure markdown-to-HTML-fragment core. No Tauri, no app protocol URLs.
//! The app (`src-tauri`) and the marketing site both call this so the site
//! demo renders byte-identical markup to the shipped app.

#[cfg(feature = "wasm")]
mod wasm;

pub mod callout;

use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

/// Maps a file reference written in a document to a URL the host can serve.
///
/// Returning `None` leaves the reference exactly as authored. The crate stays
/// dependency-free and knows nothing about the app protocol: `src-tauri`
/// supplies the closure, and the wasm entry point supplies none, so the site
/// renders references as plain content.
pub type AssetResolver<'a> = &'a dyn Fn(&str) -> Option<String>;

/// What one `[[…]]` renders as.
pub struct WikilinkRender {
    /// Where the link points. `None` leaves the link without a destination.
    pub href: Option<String>,
    /// The text the link shows: its alias when it has one, otherwise the
    /// target as it was written.
    pub label: String,
    /// True when the target names exactly one note. A target that names none,
    /// or more than one, renders as text rather than as a link.
    pub resolved: bool,
}

/// Answers what the inside of a `[[…]]` points at.
///
/// The crate stays dependency-free and knows nothing about the notes index:
/// `src-tauri` supplies the implementation and the wasm entry point supplies
/// none, so the site renders a wikilink as the text it was written as.
pub trait WikilinkResolver {
    /// Resolves the text between the brackets, alias and heading included.
    fn resolve(&self, inner: &str) -> WikilinkRender;
}

/// How deep one document's note embeds are followed before the next one
/// renders as a link instead.
///
/// Three is enough for a note that embeds a note that embeds a note, and it
/// bounds the work a document can ask for however the notes are wired
/// together. The cut is enforced here rather than left to the resolver, so a
/// resolver that ignores its `depth` argument still cannot recurse.
pub const MAX_EMBED_DEPTH: u8 = 3;

/// The note one `![[…]]` names, once the host has found it.
pub struct EmbedTarget {
    /// The note's identity, stable across every way of naming it. Two
    /// resolutions of the same note carry the same key, which is what the
    /// cycle check compares.
    pub key: String,
    /// What the embed shows when it renders as a link rather than as content:
    /// the target's alias when it has one, otherwise the name as written.
    pub label: String,
    /// Where that link points. `None` leaves it without a destination.
    pub href: Option<String>,
}

/// What one `![[…]]` naming a note points at.
///
/// The first three states are the ones [`WikilinkResolver`] carries, under the
/// same rule: a target that names several notes picks none of them. The other
/// two are facts a link never has to report — the note was found and its text
/// was deliberately not read, either because its bytes are not on this machine
/// or because rendering it would repeat work already on the page.
pub enum EmbedResolution {
    /// One note, and the Markdown it holds.
    Resolved {
        target: EmbedTarget,
        /// The whole note, frontmatter included. A `#Heading` in the embed is
        /// applied here, not by the host, so the slice is testable without a
        /// filesystem.
        markdown: String,
    },
    /// One note, whose bytes are not on this machine. The host reports this
    /// instead of reading, so an embed of an evicted note never asks a sync
    /// provider to fetch it (ADR-028 §5).
    NotDownloaded { target: EmbedTarget },
    /// One note the host chose not to read because this render is already at
    /// [`MAX_EMBED_DEPTH`] or is already inside that note.
    Cut { target: EmbedTarget },
    /// Several notes answer to the name.
    Ambiguous,
    /// No note answers to the name.
    Missing,
}

/// Answers what one `![[…]]` naming a note points at.
///
/// `depth` is how many embeds deep the render already is and `visited` holds
/// the keys of the notes it is inside, outermost first. Both are passed so an
/// implementation can decline to read a file whose text would be thrown away;
/// declining is [`EmbedResolution::Cut`], and the crate applies the same two
/// limits to a [`EmbedResolution::Resolved`] it gets back regardless.
pub trait NoteEmbedResolver {
    fn resolve(&self, target: &str, depth: u8, visited: &[&str]) -> EmbedResolution;
}

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
    render_markdown_fragment_with(text, None, None, None)
}

/// [`render_markdown_fragment`] with an optional file resolver, an optional
/// wikilink resolver and an optional note-embed resolver.
///
/// When a file resolver is supplied, three reference forms are rewritten to
/// the URL it returns: the Markdown image `![](img.png)`, the Obsidian embed
/// `![[img.png]]`, and a `src` attribute on a raw `<img>` tag. A reference
/// the resolver declines is left as authored.
///
/// When a wikilink resolver is supplied, `[[…]]` outside code becomes an
/// anchor for a target that names one note and a plain span for one that names
/// none or several.
///
/// When a note-embed resolver is supplied, `![[Note]]` and `![[Note#Heading]]`
/// outside code become a section holding that note, or that one heading of it,
/// rendered by this same function. `![[img.png]]` stays an image either way.
///
/// Callouts need no resolver and render the same for every caller.
///
/// Without any of the three the output is byte-identical to what the crate has
/// always produced for a document that carries no callout, which is what keeps
/// the site demo and the app in step.
pub fn render_markdown_fragment_with(
    text: &str,
    asset_url: Option<AssetResolver<'_>>,
    wikilinks: Option<&dyn WikilinkResolver>,
    embeds: Option<&dyn NoteEmbedResolver>,
) -> MarkdownFragment {
    render_fragment(text, asset_url, wikilinks, embeds, 0, &[])
}

/// [`render_markdown_fragment_with`] plus where this render sits in a chain of
/// note embeds: how many deep it already is, and the keys of the notes it is
/// already inside.
fn render_fragment(
    text: &str,
    asset_url: Option<AssetResolver<'_>>,
    wikilinks: Option<&dyn WikilinkResolver>,
    embeds: Option<&dyn NoteEmbedResolver>,
    depth: u8,
    visited: &[String],
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
    let mut in_code_block = false;
    let mut mermaid_src = String::new();
    // Where in `events` a rendered embed section landed. The pass that lifts a
    // section out of the paragraph it was written in reads these rather than
    // recognising the markup, so it acts on exactly what this loop produced.
    let mut sections: Vec<usize> = Vec::new();
    // Consecutive text, gathered so a `[[…]]` the parser reports as five
    // separate text events (`[`, `[`, the target, `]`, `]`) is found whole.
    // Anything that is not text flushes it first, which keeps the event order
    // exactly as the parser produced it.
    let mut pending = String::new();
    let scan = Scan {
        asset_url,
        wikilinks,
        embeds,
        depth,
        visited,
    };
    for event in parser {
        if let Event::Text(ref chunk) = event {
            if scan.is_on() && !in_mermaid && !in_metadata && !in_code_block {
                pending.push_str(chunk);
                continue;
            }
        }
        flush_inline(&mut pending, &mut events, &mut sections, &scan);
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
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                events.push(event);
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                events.push(event);
            }
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
    flush_inline(&mut pending, &mut events, &mut sections, &scan);
    let events = lift_sections(events, &sections);
    let events = wrap_callouts(events);
    let mut html_out = String::with_capacity(source.len() * 3 / 2);
    html::push_html(&mut html_out, events.into_iter());
    MarkdownFragment {
        html: html_out,
        has_mermaid,
        has_math,
    }
}

/// Everything the text scan needs to turn one `[[…]]` or `![[…]]` into markup.
struct Scan<'a> {
    asset_url: Option<AssetResolver<'a>>,
    wikilinks: Option<&'a dyn WikilinkResolver>,
    embeds: Option<&'a dyn NoteEmbedResolver>,
    /// How many note embeds deep the document being rendered already is.
    depth: u8,
    /// The keys of the notes this render is inside, outermost first.
    visited: &'a [String],
}

impl Scan<'_> {
    /// True when there is a reference form to look for at all. Without one the
    /// text goes through as the parser reported it.
    fn is_on(&self) -> bool {
        self.wikilinks.is_some() || self.embeds.is_some()
    }
}

/// Split gathered text into its plain runs, its wikilinks and its note embeds,
/// pushing all three onto `events` in the order they were written and
/// recording where each embed section landed.
///
/// Without either resolver the text goes back as one event, which is the whole
/// of the site's behaviour: a wikilink stays the characters it was typed as.
fn flush_inline<'a>(
    pending: &mut String,
    events: &mut Vec<Event<'a>>,
    sections: &mut Vec<usize>,
    scan: &Scan<'_>,
) {
    if pending.is_empty() {
        return;
    }
    let text = std::mem::take(pending);
    if !scan.is_on() {
        events.push(Event::Text(CowStr::from(text)));
        return;
    }

    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut plain_from = 0;
    while let Some(offset) = text[cursor..].find("[[") {
        let open = cursor + offset;
        let Some(len) = text[open + 2..].find("]]") else {
            break;
        };
        let inner = &text[open + 2..open + 2 + len];
        // A nested `[` is not a target, and `[[]]` names nothing.
        if inner.trim().is_empty() || inner.contains('[') {
            cursor = open + 2;
            continue;
        }
        let is_embed = open > 0 && bytes[open - 1] == b'!';
        let rendered = match is_embed {
            // An embed naming an image file is an image wherever it is
            // rewritten, so it is never read as a note.
            true if image_embed(inner).is_some() => None,
            true => scan
                .embeds
                .map(|resolver| embed_markup(inner, resolver, scan)),
            false => scan
                .wikilinks
                .map(|resolver| Markup::inline(wikilink_html(&resolver.resolve(inner)))),
        };
        let Some(rendered) = rendered else {
            cursor = open + 2;
            continue;
        };
        // The `!` belongs to the embed, not to the text before it.
        let plain_to = if is_embed { open - 1 } else { open };
        if plain_from < plain_to {
            events.push(Event::Text(CowStr::from(
                text[plain_from..plain_to].to_string(),
            )));
        }
        match rendered {
            Markup::Inline(html) => events.push(Event::InlineHtml(CowStr::from(html))),
            Markup::Section(html) => {
                sections.push(events.len());
                events.push(Event::Html(CowStr::from(html)));
            }
        }
        cursor = open + 2 + len + 2;
        plain_from = cursor;
    }
    if plain_from < text.len() {
        events.push(Event::Text(CowStr::from(text[plain_from..].to_string())));
    }
}

/// One rendered reference and whether it stands on its own.
enum Markup {
    /// Phrasing content, which stays in the paragraph it was written in.
    Inline(String),
    /// A block, which is lifted out of that paragraph.
    Section(String),
}

impl Markup {
    fn inline(html: String) -> Self {
        Markup::Inline(html)
    }
}

/// The markup one `![[…]]` naming a note becomes.
fn embed_markup(inner: &str, resolver: &dyn NoteEmbedResolver, scan: &Scan<'_>) -> Markup {
    let borrowed: Vec<&str> = scan.visited.iter().map(String::as_str).collect();
    let resolution = resolver.resolve(inner, scan.depth, &borrowed);
    let (target, markdown) = match resolution {
        // A target that names several notes picks none of them, and one that
        // names no note has nothing to show. Both read as text, exactly as the
        // link form of the same target does.
        EmbedResolution::Ambiguous | EmbedResolution::Missing => {
            return Markup::Inline(missing_embed_html(inner))
        }
        EmbedResolution::Cut { target } => return Markup::Inline(embed_link_html(&target)),
        EmbedResolution::NotDownloaded { target } => {
            return Markup::Section(not_downloaded_html(inner, &target))
        }
        EmbedResolution::Resolved { target, markdown } => (target, markdown),
    };
    // The two limits are applied to what came back as well as passed to the
    // resolver, so a resolver that ignores them still cannot recurse.
    if scan.depth >= MAX_EMBED_DEPTH || scan.visited.contains(&target.key) {
        return Markup::Inline(embed_link_html(&target));
    }
    let body = match heading_section(&markdown, embed_heading(inner)) {
        Some(body) => body,
        // The note is there and the heading in the target is not, so the embed
        // names nothing to show.
        None => return Markup::Inline(missing_embed_html(inner)),
    };
    let mut visited = scan.visited.to_vec();
    visited.push(target.key);
    let rendered = render_fragment(
        &body,
        scan.asset_url,
        scan.wikilinks,
        Some(resolver),
        scan.depth + 1,
        &visited,
    );
    Markup::Section(format!(
        "<section class=\"writ-embed\" data-target=\"{}\">{}</section>",
        escape_attribute(inner.trim()),
        rendered.html
    ))
}

/// The heading a target names, or `None` when it names the whole note.
fn embed_heading(inner: &str) -> Option<&str> {
    let target = inner.split('|').next().unwrap_or(inner);
    let heading = target.split_once('#')?.1.trim();
    (!heading.is_empty()).then_some(heading)
}

/// The slice of `markdown` one heading owns: the heading line through to the
/// next heading at the same level or shallower, or the whole text when the
/// target named no heading.
///
/// `None` says the note holds no such heading. Headings are matched on their
/// trimmed text, case-folded, which is how a target names one in prose.
fn heading_section(markdown: &str, heading: Option<&str>) -> Option<String> {
    let Some(wanted) = heading else {
        return Some(markdown.to_string());
    };
    let wanted = wanted.to_lowercase();
    let body = split_frontmatter(markdown).body;
    let mut start: Option<(usize, usize)> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let end = offset + line.len();
        if let Some((level, text)) = atx_heading(line) {
            match start {
                None if text.to_lowercase() == wanted => start = Some((offset, level)),
                Some((from, opened)) if level <= opened => {
                    return Some(body[from..offset].to_string())
                }
                _ => {}
            }
        }
        offset = end;
    }
    start.map(|(from, _)| body[from..].to_string())
}

/// The level and text of an ATX heading line, or `None` for any other line.
fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_end_matches(['\n', '\r']).trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim()))
}

/// A target the index cannot name one note for, read as the text it was
/// written as.
fn missing_embed_html(inner: &str) -> String {
    format!(
        "<span class=\"writ-embed-missing\">{}</span>",
        escape_text(inner.trim())
    )
}

/// The link an embed renders as instead of repeating a note the page is
/// already inside, or going deeper than [`MAX_EMBED_DEPTH`].
fn embed_link_html(target: &EmbedTarget) -> String {
    wikilink_html(&WikilinkRender {
        href: target.href.clone(),
        label: target.label.clone(),
        resolved: target.href.is_some(),
    })
}

/// What an embed of a note whose bytes are not on this machine shows.
fn not_downloaded_html(inner: &str, target: &EmbedTarget) -> String {
    format!(
        "<section class=\"writ-embed\" data-target=\"{}\"><p class=\"writ-embed-placeholder\">\
{} is not downloaded.</p></section>",
        escape_attribute(inner.trim()),
        escape_text(&target.label)
    )
}

/// Lift each rendered embed section out of the paragraph it was written in.
///
/// A section is a block and a paragraph holds phrasing content, so a section
/// left where it was written closes the `<p>` around it in every browser and
/// the text after it lands outside. The paragraph is split instead: what came
/// before the section stays in one, what comes after goes into another, and a
/// paragraph that would hold nothing is dropped.
fn lift_sections<'a>(events: Vec<Event<'a>>, sections: &[usize]) -> Vec<Event<'a>> {
    if sections.is_empty() {
        return events;
    }
    let mut out: Vec<Event<'a>> = Vec::with_capacity(events.len() + sections.len() * 2);
    // Where the paragraph currently being read starts in `out`, when one is
    // open. A section closes it and a new one opens after.
    let mut paragraph: Option<usize> = None;
    let mut split = false;
    for (index, event) in events.into_iter().enumerate() {
        match event {
            Event::Start(Tag::Paragraph) => {
                paragraph = Some(out.len());
                split = false;
                out.push(event);
            }
            Event::End(TagEnd::Paragraph) => {
                match (paragraph, split) {
                    // Nothing was written after the last section, so the
                    // paragraph reopened for it holds nothing.
                    (Some(at), true) if at + 1 == out.len() => {
                        out.pop();
                    }
                    _ => out.push(event),
                }
                paragraph = None;
            }
            _ if sections.binary_search(&index).is_ok() => {
                if let Some(at) = paragraph {
                    if at + 1 == out.len() {
                        // The section is the whole of the paragraph so far;
                        // there is nothing to keep in front of it.
                        out.pop();
                    } else {
                        out.push(Event::End(TagEnd::Paragraph));
                    }
                    split = true;
                }
                out.push(event);
                if paragraph.is_some() {
                    paragraph = Some(out.len());
                    out.push(Event::Start(Tag::Paragraph));
                }
            }
            _ => out.push(event),
        }
    }
    out
}

/// Turn every blockquote that opens with `[!type]` into a callout.
///
/// The blockquote's own content is left exactly as the parser produced it, so
/// a table, a math span or a mermaid fence inside a callout renders through
/// the same events it would anywhere else, and a callout nested in a list or
/// in another blockquote is found by the same rule as one at the top level.
fn wrap_callouts(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    if !events
        .iter()
        .any(|event| matches!(event, Event::Start(Tag::BlockQuote(_))))
    {
        return events;
    }
    let mut out: Vec<Event> = Vec::with_capacity(events.len());
    // One entry per open blockquote: true when it is a callout, so the right
    // `</blockquote>` is the one that closes it.
    let mut open: Vec<bool> = Vec::new();
    let mut index = 0;
    while index < events.len() {
        match &events[index] {
            Event::Start(Tag::BlockQuote(_)) => {
                let Some((callout, texts)) = header_at(&events, index) else {
                    open.push(false);
                    out.push(events[index].clone());
                    index += 1;
                    continue;
                };
                open.push(true);
                out.push(Event::Html(CowStr::from(callout::open_html(&callout))));
                // Past the blockquote start, the paragraph the header opened
                // and the text events the header was reported as.
                index += 2 + texts;
                match events.get(index) {
                    // The header's line ran on into the body, which keeps the
                    // paragraph it was written in.
                    Some(Event::SoftBreak) => {
                        index += 1;
                        out.push(Event::Start(Tag::Paragraph));
                    }
                    // The header was the whole paragraph.
                    Some(Event::End(TagEnd::Paragraph)) => index += 1,
                    // Anything else on the header's own line is body text.
                    _ => out.push(Event::Start(Tag::Paragraph)),
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                match open.pop() {
                    Some(true) => out.push(Event::Html(CowStr::from(callout::close_html()))),
                    _ => out.push(events[index].clone()),
                }
                index += 1;
            }
            _ => {
                out.push(events[index].clone());
                index += 1;
            }
        }
    }
    out
}

/// The callout the blockquote starting at `index` opens, and how many text
/// events its header was reported as.
///
/// The header's line arrives as several text events, because `[` opens a link
/// reference the parser reports separately, so the run of them is joined
/// before it is read. Only the text of a first paragraph counts: a blockquote
/// whose first block is a list or a fence carries no header, whatever it says.
fn header_at(events: &[Event<'_>], index: usize) -> Option<(callout::Callout, usize)> {
    match events.get(index + 1) {
        Some(Event::Start(Tag::Paragraph)) => {}
        _ => return None,
    }
    let mut line = String::new();
    let mut texts = 0;
    while let Some(Event::Text(text)) = events.get(index + 2 + texts) {
        line.push_str(text);
        texts += 1;
    }
    callout::parse(&line).map(|callout| (callout, texts))
}

/// Escape a value going into a double-quoted HTML attribute.
fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;")
}

/// The markup one resolved or unresolved wikilink becomes.
///
/// A resolved target is an anchor and an unresolved one is a span, so a link
/// to a note that is not there yet reads as text rather than as a destination
/// that goes nowhere.
fn wikilink_html(render: &WikilinkRender) -> String {
    let label = escape_text(&render.label);
    match (&render.href, render.resolved) {
        (Some(href), true) => format!(
            "<a class=\"writ-wikilink\" href=\"{}\">{label}</a>",
            escape_attribute(href)
        ),
        _ => format!("<span class=\"writ-wikilink writ-wikilink-missing\">{label}</span>"),
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
        let body = &raw[name_end..];
        let tag = scan_img_tag(body);
        // An unterminated tag is left exactly as authored: there is no tag
        // to rewrite until its `>` arrives.
        if !tag.terminated {
            break;
        }
        out.push_str(&raw[cursor..name_end]);
        match tag
            .src
            .and_then(|value| resolve(&body[value.clone()]).map(|url| (value, url)))
        {
            Some((value, url)) => {
                out.push_str(&body[..value.start]);
                out.push_str(&url);
                out.push_str(&body[value.end..tag.end]);
                rewrote = true;
            }
            None => out.push_str(&body[..tag.end]),
        }
        cursor = name_end + tag.end;
    }
    if !rewrote {
        return raw;
    }
    out.push_str(&raw[cursor..]);
    CowStr::from(out)
}

/// What one scan of a tag body found.
struct ImgTag {
    /// Byte offset of the tag's closing `>` within the body.
    end: usize,
    /// False when the body holds no closing `>` at all.
    terminated: bool,
    /// Byte range of the value of the tag's own `src` attribute.
    src: Option<core::ops::Range<usize>>,
}

/// Walk one `<img>` tag body attribute by attribute.
///
/// Quoting is tracked, so `src` is found only where it is the tag's own
/// attribute name: the same letters inside another attribute's value
/// (`alt="a src=x"`, `data-src=…`) are part of that value, and a `>` inside a
/// quoted value does not end the tag. Attribute names are matched
/// case-insensitively, and the value may be double-quoted, single-quoted or
/// bare, as the HTML tokenizer reads them.
fn scan_img_tag(body: &str) -> ImgTag {
    let bytes = body.as_bytes();
    let mut src: Option<core::ops::Range<usize>> = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'>' => {
                return ImgTag {
                    end: i,
                    terminated: true,
                    src,
                }
            }
            b'/' => i += 1,
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let name_start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(bytes[i], b'=' | b'>' | b'/')
                {
                    i += 1;
                }
                let name = &body[name_start..i];
                let mut after = i;
                while after < bytes.len() && bytes[after].is_ascii_whitespace() {
                    after += 1;
                }
                // A bare attribute (`hidden`) has no value to skip past.
                if bytes.get(after) != Some(&b'=') {
                    continue;
                }
                after += 1;
                while after < bytes.len() && bytes[after].is_ascii_whitespace() {
                    after += 1;
                }
                let (value, next) = match bytes.get(after) {
                    Some(quote @ (b'"' | b'\'')) => {
                        let start = after + 1;
                        match body[start..].find(*quote as char) {
                            Some(len) => (start..start + len, start + len + 1),
                            None => (start..body.len(), body.len()),
                        }
                    }
                    Some(_) => {
                        let len = body[after..]
                            .find(|c: char| c.is_ascii_whitespace() || c == '>')
                            .unwrap_or(body.len() - after);
                        (after..after + len, after + len)
                    }
                    None => (body.len()..body.len(), body.len()),
                };
                if src.is_none() && name.eq_ignore_ascii_case("src") {
                    src = Some(value);
                }
                i = next;
            }
        }
    }
    ImgTag {
        end: bytes.len(),
        terminated: false,
        src,
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
        render_markdown_fragment_with(text, Some(&served), None, None).html
    }

    /// Resolver standing in for the notes index: every name but `Missing`
    /// names one note, and `Both` names two.
    struct Notes;

    impl WikilinkResolver for Notes {
        fn resolve(&self, inner: &str) -> WikilinkRender {
            let (target, alias) = match inner.split_once('|') {
                Some((target, alias)) => (target, Some(alias.trim().to_string())),
                None => (inner, None),
            };
            let name = target.split_once('#').map_or(target, |(name, _)| name);
            let found = name.trim() != "Missing" && name.trim() != "Both";
            WikilinkRender {
                href: found.then(|| format!("{}.md", name.trim())),
                label: alias.unwrap_or_else(|| target.trim().to_string()),
                resolved: found,
            }
        }
    }

    fn with_wikilinks(text: &str) -> String {
        render_markdown_fragment_with(text, None, Some(&Notes), None).html
    }

    #[test]
    fn a_resolved_wikilink_is_an_anchor() {
        let html = with_wikilinks("see [[Note]] here");
        assert!(html.contains("<a class=\"writ-wikilink\" href=\"Note.md\">Note</a>"));
        assert!(html.contains("see "));
        assert!(html.contains(" here"));
    }

    #[test]
    fn an_unresolved_wikilink_is_a_plain_span() {
        let html = with_wikilinks("see [[Missing]] here");
        assert!(html.contains("<span class=\"writ-wikilink writ-wikilink-missing\">Missing</span>"));
        assert!(!html.contains("<a "));
    }

    #[test]
    fn an_ambiguous_wikilink_is_a_plain_span() {
        let html = with_wikilinks("[[Both]]");
        assert!(html.contains("writ-wikilink-missing"));
        assert!(!html.contains("<a "));
    }

    #[test]
    fn an_alias_is_what_the_link_shows() {
        let html = with_wikilinks("[[Note|the note]]");
        assert!(html.contains(">the note</a>"));
        assert!(html.contains("href=\"Note.md\""));
    }

    #[test]
    fn a_heading_target_still_resolves_the_note() {
        let html = with_wikilinks("[[Note#Section]]");
        assert!(html.contains("href=\"Note.md\""));
        assert!(html.contains(">Note#Section</a>"));
    }

    #[test]
    fn two_wikilinks_on_one_line_are_both_found() {
        let html = with_wikilinks("[[Note]] and [[Other]]");
        assert!(html.contains("href=\"Note.md\""));
        assert!(html.contains("href=\"Other.md\""));
        assert!(html.contains(" and "));
    }

    #[test]
    fn a_wikilink_inside_code_stays_literal() {
        let fenced = with_wikilinks("```\n[[Note]]\n```");
        assert!(fenced.contains("[[Note]]"));
        assert!(!fenced.contains("writ-wikilink"));

        let inline = with_wikilinks("use `[[Note]]` for a link");
        assert!(inline.contains("[[Note]]"));
        assert!(!inline.contains("writ-wikilink"));
    }

    #[test]
    fn a_note_embed_is_not_a_wikilink() {
        let html = with_wikilinks("![[Some Note]]");
        assert!(html.contains("![[Some Note]]"));
        assert!(!html.contains("writ-wikilink"));
    }

    #[test]
    fn a_label_is_escaped_not_injected() {
        let html = with_wikilinks("[[Note|a & b]]");
        assert!(html.contains(">a &amp; b</a>"));
    }

    /// Raw HTML is its own event, so a tag inside the brackets ends the run
    /// of text the target would have been found in. The characters stay as
    /// authored rather than becoming half a link.
    #[test]
    fn a_tag_inside_the_brackets_is_not_a_link() {
        let html = with_wikilinks("[[Note|<b>x</b>]]");
        assert!(!html.contains("writ-wikilink"));
    }

    #[test]
    fn frontmatter_is_still_hidden_with_a_resolver() {
        let html = with_wikilinks("---\ntitle: [[Note]]\n---\n\n[[Note]]");
        assert_eq!(html.matches("writ-wikilink").count(), 1);
        assert!(!html.contains("title:"));
    }

    #[test]
    fn without_a_resolver_a_wikilink_is_untouched() {
        let html = render_markdown_fragment("[[Note]]").html;
        assert!(html.contains("[[Note]]"));
        assert!(!html.contains("writ-wikilink"));
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
    fn only_the_tag_s_own_src_attribute_is_rewritten() {
        // `src` inside another attribute's value is part of that value.
        let alt = with_assets("<img alt=\"a src=x\" src=\"real.png\">");
        assert!(alt.contains("alt=\"a src=x\""));
        assert!(alt.contains("src=\"writ-preview://document/_note-asset/b/n/real.png\""));

        // A different attribute that ends in `src` is not the `src` attribute.
        let data = with_assets("<img data-src=\"decoy.png\" src=\"real.png\">");
        assert!(data.contains("data-src=\"decoy.png\""));
        assert!(data.contains("src=\"writ-preview://document/_note-asset/b/n/real.png\""));
        assert!(!data.contains("_note-asset/b/n/decoy.png"));

        // …and on its own it names no image the preview serves.
        let only_data = with_assets("<img data-src=\"decoy.png\">");
        assert!(!only_data.contains("_note-asset"));
    }

    #[test]
    fn a_quoted_value_may_hold_the_character_that_ends_a_tag() {
        let html = with_assets("<img alt=\"a>b\" src=\"a.png\">");
        assert!(html.contains("alt=\"a>b\""));
        assert!(html.contains("src=\"writ-preview://document/_note-asset/b/n/a.png\""));
    }

    #[test]
    fn the_src_attribute_is_found_however_it_is_written() {
        let bare = with_assets("<img src=a.png width=20>");
        assert!(bare.contains("src=writ-preview://document/_note-asset/b/n/a.png"));
        assert!(bare.contains("width=20"));

        let upper = with_assets("<IMG SRC=\"a.png\">");
        assert!(upper.contains("SRC=\"writ-preview://document/_note-asset/b/n/a.png\""));

        let spaced = with_assets("<img src = 'a.png' >");
        assert!(spaced.contains("src = 'writ-preview://document/_note-asset/b/n/a.png'"));
    }

    #[test]
    fn an_unterminated_tag_is_left_as_authored() {
        let html = with_assets("<img src=\"a.png\"");
        assert!(!html.contains("_note-asset"));
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

    /// The document the byte-identity pin renders, and what the crate rendered
    /// for it before callouts and note embeds existed.
    ///
    /// It carries a table, math, a mermaid fence, every image reference form,
    /// wikilinks and frontmatter, and no callout and no embed, so a change to
    /// either of those that reaches a document using neither shows up here as
    /// a diff.
    const PLAIN_DOCUMENT: &str = include_str!("../tests/fixtures/plain-document.md");
    const PLAIN_NO_RESOLVER: &str =
        include_str!("../tests/fixtures/plain-document.no-resolver.html");
    const PLAIN_RESOLVED: &str = include_str!("../tests/fixtures/plain-document.resolved.html");

    #[test]
    fn a_document_with_no_callout_and_no_embed_renders_unchanged() {
        assert_eq!(
            render_markdown_fragment(PLAIN_DOCUMENT).html,
            PLAIN_NO_RESOLVER
        );
    }

    #[test]
    fn a_document_with_no_callout_and_no_embed_renders_unchanged_with_resolvers() {
        let html =
            render_markdown_fragment_with(PLAIN_DOCUMENT, Some(&served), Some(&Notes), None).html;
        assert_eq!(html, PLAIN_RESOLVED);
    }

    /// Resolver standing in for the notes folder: a fixed set of notes by
    /// name, `Both` naming two of them and everything else naming none.
    /// `Evicted` names one note whose bytes are not on this machine.
    struct Folder;

    /// What each note in the stand-in folder holds.
    fn note_source(name: &str) -> Option<&'static str> {
        match name {
            "A" => Some("A body\n\n![[B]]\n"),
            "B" => Some("B body\n\n![[A]]\n"),
            "Chain3" => Some("Chain3 body\n"),
            "Deep1" => Some("Deep1 body\n\n![[Deep2]]\n"),
            "Deep2" => Some("Deep2 body\n\n![[Deep3]]\n"),
            "Deep3" => Some("Deep3 body\n\n![[Deep4]]\n"),
            "Deep4" => Some("Deep4 body\n"),
            "Quo\"te" => Some("Quoted body\n"),
            "Note" => Some(
                "---\ntitle: Note\n---\n\nLead paragraph.\n\n## First\n\nFirst body.\n\n### Under first\n\nDeeper.\n\n## Second\n\nSecond body.\n",
            ),
            _ => None,
        }
    }

    impl NoteEmbedResolver for Folder {
        fn resolve(&self, target: &str, depth: u8, visited: &[&str]) -> EmbedResolution {
            let name = target
                .split('|')
                .next()
                .unwrap_or(target)
                .split('#')
                .next()
                .unwrap_or(target)
                .trim();
            let known = |name: &str| EmbedTarget {
                key: name.to_string(),
                label: name.to_string(),
                href: Some(format!("{name}.md")),
            };
            match name {
                "Both" => EmbedResolution::Ambiguous,
                "Evicted" => EmbedResolution::NotDownloaded {
                    target: known("Evicted"),
                },
                _ => match note_source(name) {
                    None => EmbedResolution::Missing,
                    // The read is skipped for a target this render would cut
                    // anyway, which is the whole reason the two are passed in.
                    Some(_) if depth >= MAX_EMBED_DEPTH || visited.contains(&name) => {
                        EmbedResolution::Cut {
                            target: known(name),
                        }
                    }
                    Some(markdown) => EmbedResolution::Resolved {
                        target: known(name),
                        markdown: markdown.to_string(),
                    },
                },
            }
        }
    }

    /// A resolver that always answers with the note it is asked for, ignoring
    /// both limits, so the crate's own enforcement is what is under test.
    struct Ignores;

    impl NoteEmbedResolver for Ignores {
        fn resolve(&self, target: &str, _depth: u8, _visited: &[&str]) -> EmbedResolution {
            let name = target.split('#').next().unwrap_or(target).trim();
            match note_source(name) {
                None => EmbedResolution::Missing,
                Some(markdown) => EmbedResolution::Resolved {
                    target: EmbedTarget {
                        key: name.to_string(),
                        label: name.to_string(),
                        href: Some(format!("{name}.md")),
                    },
                    markdown: markdown.to_string(),
                },
            }
        }
    }

    fn with_embeds(text: &str) -> String {
        render_markdown_fragment_with(text, None, Some(&Notes), Some(&Folder)).html
    }

    // Turning the embed resolver on is what puts the pass that lifts a section
    // out of its paragraph in the way of every document, so a document that
    // embeds nothing has to come out of it byte for byte.
    #[test]
    fn a_document_with_no_embed_renders_unchanged_with_the_embed_resolver_on() {
        let html = render_markdown_fragment_with(
            PLAIN_DOCUMENT,
            Some(&served),
            Some(&Notes),
            Some(&Folder),
        )
        .html;
        assert_eq!(html, PLAIN_RESOLVED);
    }

    #[test]
    fn every_callout_type_renders_with_its_own_data_callout() {
        for kind in [
            "note", "abstract", "info", "todo", "tip", "success", "question", "warning", "failure",
            "danger", "bug", "example", "quote",
        ] {
            let html = render_markdown_fragment(&format!("> [!{kind}]\n> body\n")).html;
            assert!(
                html.contains(&format!("data-callout=\"{kind}\"")),
                "{kind}: {html}"
            );
            assert!(
                html.contains(&format!("data-callout-type=\"{kind}\"")),
                "{kind}"
            );
            assert!(html.contains("<p>body</p>"), "{kind}: {html}");
            assert!(!html.contains("<blockquote>"), "{kind}: {html}");
        }
    }

    #[test]
    fn a_fold_marker_and_a_title_reach_the_markup() {
        let html = render_markdown_fragment("> [!tip]- Folded title\n> body\n").html;
        assert!(html.contains("data-callout=\"tip\""));
        assert!(html.contains("data-fold=\"closed\""));
        assert!(html.contains("<div class=\"writ-callout-title\">Folded title</div>"));
        assert!(html.contains("<p>body</p>"));

        let open = render_markdown_fragment("> [!tip]+ Open title\n> body\n").html;
        assert!(open.contains("data-fold=\"open\""));
        assert!(render_markdown_fragment("> [!tip]\n> body\n")
            .html
            .contains("data-fold=\"none\""));
    }

    #[test]
    fn a_callout_type_the_crate_does_not_know_falls_back_and_keeps_the_word() {
        let html = render_markdown_fragment("> [!spaceship] Title\n> body\n").html;
        assert!(html.contains("data-callout=\"spaceship\""));
        assert!(html.contains("data-callout-type=\"note\""));
    }

    #[test]
    fn a_callout_alias_resolves_to_its_type() {
        let html = render_markdown_fragment("> [!tldr]\n> body\n").html;
        assert!(html.contains("data-callout=\"tldr\""));
        assert!(html.contains("data-callout-type=\"abstract\""));
        assert!(html.contains("<div class=\"writ-callout-title\">Tldr</div>"));
    }

    #[test]
    fn a_callout_nested_in_a_list_is_still_a_callout() {
        let html = render_markdown_fragment("- item\n\n  > [!warning] Careful\n  > body\n").html;
        assert!(html.contains("<ul>"));
        assert!(html.contains("data-callout=\"warning\""), "{html}");
        assert!(html.contains("<p>body</p>"), "{html}");
    }

    #[test]
    fn a_callout_nested_in_a_blockquote_is_still_a_callout() {
        let html = render_markdown_fragment("> outer\n>\n> > [!info] Inner\n> > body\n").html;
        assert!(html.contains("<blockquote>"), "{html}");
        assert!(html.contains("data-callout=\"info\""), "{html}");
        assert!(html.contains("<p>outer</p>"), "{html}");
        assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
        assert_eq!(html.matches("</blockquote>").count(), 1, "{html}");
    }

    #[test]
    fn a_blockquote_with_no_marker_stays_a_blockquote() {
        let html = render_markdown_fragment("> just a quotation\n").html;
        assert!(html.contains("<blockquote>"));
        assert!(!html.contains("writ-callout"));
    }

    #[test]
    fn a_callout_still_renders_a_table_a_math_span_and_a_mermaid_fence() {
        let fragment = render_markdown_fragment(
            "> [!example] Everything\n> \n> | a | b |\n> | --- | --- |\n> | 1 | 2 |\n> \n> Value $x^2$ here.\n> \n> ```mermaid\n> graph TD\n>   A --> B\n> ```\n",
        );
        let html = &fragment.html;
        assert!(html.contains("data-callout=\"example\""), "{html}");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("<pre class=\"mermaid\">"), "{html}");
        assert!(html.contains("x^2"), "{html}");
        assert!(fragment.has_mermaid);
        assert!(fragment.has_math);
    }

    #[test]
    fn a_note_embed_becomes_a_section_holding_the_note() {
        let html = with_embeds("![[Chain3]]\n");
        assert!(
            html.contains("<section class=\"writ-embed\" data-target=\"Chain3\">"),
            "{html}"
        );
        assert!(html.contains("<p>Chain3 body</p>"), "{html}");
        // A section is a block, so it never sits inside the paragraph it was
        // written in.
        assert!(!html.contains("<p><section"), "{html}");
    }

    #[test]
    fn text_around_an_embed_keeps_its_place() {
        let html = with_embeds("before ![[Chain3]] after\n");
        assert!(html.starts_with("<p>before </p>"), "{html}");
        assert!(html.contains("<p> after</p>"), "{html}");
        assert!(!html.contains("<p></p>"), "{html}");
    }

    #[test]
    fn an_embed_of_one_heading_renders_that_heading_only() {
        let html = with_embeds("![[Note#First]]\n");
        assert!(html.contains("data-target=\"Note#First\""), "{html}");
        assert!(html.contains("<h2>First</h2>"), "{html}");
        assert!(html.contains("First body"), "{html}");
        // The deeper heading belongs to the section; the next one at the same
        // level does not, and neither does what came before it.
        assert!(html.contains("<h3>Under first</h3>"), "{html}");
        assert!(!html.contains("Second body"), "{html}");
        assert!(!html.contains("Lead paragraph"), "{html}");
    }

    #[test]
    fn an_embed_of_a_heading_the_note_does_not_have_is_a_plain_span() {
        let html = with_embeds("![[Note#Nowhere]]\n");
        assert!(
            html.contains("<span class=\"writ-embed-missing\">Note#Nowhere</span>"),
            "{html}"
        );
        assert!(!html.contains("<section"), "{html}");
    }

    #[test]
    fn a_cycle_stops_at_the_note_the_page_is_already_inside_and_that_point_is_a_link() {
        let html = with_embeds("![[A]]\n");
        assert!(html.contains("<p>A body</p>"), "{html}");
        assert!(html.contains("<p>B body</p>"), "{html}");
        assert_eq!(html.matches("A body").count(), 1, "{html}");
        assert_eq!(html.matches("<section").count(), 2, "{html}");
        assert!(
            html.contains("<a class=\"writ-wikilink\" href=\"A.md\">A</a>"),
            "{html}"
        );
    }

    // A chain four notes long, through a resolver that answers every question
    // and reads neither its depth nor its visited set: three embeds render and
    // the fourth is a link.
    #[test]
    fn an_embed_chain_is_cut_three_deep_whatever_the_resolver_answers() {
        let html = render_markdown_fragment_with("![[Deep1]]\n", None, None, Some(&Ignores)).html;
        assert_eq!(html.matches("<section").count(), 3, "{html}");
        assert!(html.contains("<p>Deep3 body</p>"), "{html}");
        assert!(!html.contains("Deep4 body"), "{html}");
        assert!(
            html.contains("<a class=\"writ-wikilink\" href=\"Deep4.md\">Deep4</a>"),
            "{html}"
        );
    }

    #[test]
    fn an_ambiguous_embed_target_is_a_span_not_an_anchor() {
        let html = with_embeds("![[Both]]\n");
        assert!(
            html.contains("<span class=\"writ-embed-missing\">Both</span>"),
            "{html}"
        );
        assert!(!html.contains("<a "), "{html}");
        assert!(!html.contains("<section"), "{html}");
    }

    #[test]
    fn a_missing_embed_target_is_a_span() {
        let html = with_embeds("![[Nowhere]]\n");
        assert!(
            html.contains("<span class=\"writ-embed-missing\">Nowhere</span>"),
            "{html}"
        );
        assert!(!html.contains("<section"), "{html}");
    }

    #[test]
    fn a_target_that_is_not_downloaded_renders_the_placeholder() {
        let html = with_embeds("![[Evicted]]\n");
        assert!(
            html.contains("<p class=\"writ-embed-placeholder\">Evicted is not downloaded.</p>"),
            "{html}"
        );
        assert!(html.contains("data-target=\"Evicted\""), "{html}");
    }

    #[test]
    fn an_image_embed_is_still_an_image_when_a_note_resolver_is_present() {
        let html = render_markdown_fragment_with(
            "![[picture.png]]\n",
            Some(&served),
            Some(&Notes),
            Some(&Folder),
        )
        .html;
        assert!(html.contains("<img src=\"writ-preview://document/_note-asset/b/n/picture.png\""));
        assert!(!html.contains("writ-embed"));
    }

    #[test]
    fn an_image_embed_with_no_asset_resolver_is_not_read_as_a_note() {
        let html = with_embeds("![[picture.png]]\n");
        assert!(html.contains("![[picture.png]]"), "{html}");
        assert!(!html.contains("writ-embed"), "{html}");
    }

    #[test]
    fn an_embed_inside_code_is_not_resolved() {
        let html = with_embeds("`![[Chain3]]`\n\n```\n![[Chain3]]\n```\n");
        assert!(!html.contains("writ-embed"), "{html}");
        assert!(!html.contains("Chain3 body"), "{html}");
    }

    #[test]
    fn without_an_embed_resolver_a_note_embed_is_untouched() {
        let html = render_markdown_fragment("![[Chain3]]\n").html;
        assert_eq!(html, "<p>![[Chain3]]</p>\n");
    }

    #[test]
    fn an_embed_target_is_escaped_wherever_it_is_written_out() {
        let missing = with_embeds("![[a & b]]\n");
        assert!(
            missing.contains("<span class=\"writ-embed-missing\">a &amp; b</span>"),
            "{missing}"
        );
        let resolved = with_embeds("![[Quo\"te]]\n");
        assert!(
            resolved.contains("data-target=\"Quo&quot;te\""),
            "{resolved}"
        );
        assert!(resolved.contains("<p>Quoted body</p>"), "{resolved}");
    }
}
