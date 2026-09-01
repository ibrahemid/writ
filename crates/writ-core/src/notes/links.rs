//! Link syntax and link resolution, in one place.
//!
//! A note links to another note by its name, not by its row or its path
//! (ADR-034). Four consumers ask the same questions of that syntax — the index
//! that stores `links`, the preview renderer, the editor, and the command line
//! — so the syntax set, the case rule and the ambiguity rule live here and
//! nowhere else. Every function is pure: nothing here opens a file or reaches a
//! database, and a candidate list is always supplied by the caller.
//!
//! # Positions
//!
//! `line` is 1-based, matching [`crate::search`]'s hits, and `col` is a 0-based
//! character offset inside that line. `byte_range` is a byte range into the
//! whole text, so a caller holding the text can slice the link back out of it.

use std::collections::HashSet;
use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

/// Extensions a link target may spell out and still mean the same note.
const NOTE_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Which syntax a link was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `[[Note]]`, with optional `|alias`, `#heading` and `folder/` prefix.
    Wikilink,
    /// `[label](path)` where the path names a note rather than a URL.
    Markdown,
}

impl LinkKind {
    /// The value stored in `links.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wikilink => "wikilink",
            Self::Markdown => "markdown",
        }
    }

    /// The kind a stored `links.kind` names, or `None` for a value no version
    /// of Writ wrote.
    pub fn from_stored(text: &str) -> Option<Self> {
        match text {
            "wikilink" => Some(Self::Wikilink),
            "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }
}

/// One link as it was written, before anything tries to resolve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLink {
    /// Which syntax it was written in.
    pub kind: LinkKind,
    /// The note part of the link: no alias, no heading, folder prefix kept.
    pub target: String,
    /// The label shown instead of the target, when the link carries one.
    pub alias: Option<String>,
    /// The heading inside the target the link points at.
    pub heading: Option<String>,
    /// 1-based line the link starts on.
    pub line: u32,
    /// 0-based character offset of the link inside that line.
    pub col: u32,
    /// Byte range of the whole link syntax inside the text it was scanned from.
    pub byte_range: Range<usize>,
}

impl RawLink {
    /// The target this link points at, split into the parts [`resolve`] reads.
    pub fn wikilink_target(&self) -> WikilinkTarget {
        let mut target = parse_target(&self.target);
        target.alias.clone_from(&self.alias);
        target.heading.clone_from(&self.heading);
        target
    }
}

/// A link target, split into the parts resolution reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WikilinkTarget {
    /// The note's name, with any note extension removed.
    pub name: String,
    /// The folder path written before the name, when the link carries one.
    pub folder: Option<String>,
    /// The heading inside the note the link points at.
    pub heading: Option<String>,
    /// The label shown instead of the target.
    pub alias: Option<String>,
}

/// What a target resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Exactly one note answers to the target.
    Resolved(String),
    /// Several notes answer to it and the ordering did not separate them. The
    /// candidates are handed back for the user to pick from; a best guess here
    /// is what silently rewrites the wrong file during a rename.
    Ambiguous(Vec<String>),
    /// No note answers to it.
    Missing,
}

/// Splits a leading YAML frontmatter block off `text` by byte offset.
///
/// Semantics match [`crate::prompt::strip::strip_frontmatter`] and
/// `writ_render::split_frontmatter`: the first line must be exactly `---` after
/// trimming line-end whitespace, and a later line must be exactly `---`. An
/// unterminated or malformed block is body text, never swallowed.
///
/// # Why a third copy
///
/// The index writes these facts from inside `writ-storage`, which cannot reach
/// `writ-render`: that crate is a leaf compiled to wasm for the site, and
/// pulling it in through `writ-core` would put `uuid` and `chrono` into the
/// site's bundle. The prompt stripper next door returns only the body, and a
/// property parser needs the block itself. So the rule is written out a third
/// time, and `frontmatter_divergence_tests` asserts all three agree byte for
/// byte over one shared table.
pub fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (None, text);
    };
    if first.trim_end() != "---" {
        return (None, text);
    }
    let mut offset = first.len();
    for line in lines {
        let end = offset + line.len();
        if line.trim_end() == "---" {
            return (Some(&text[..end]), &text[end..]);
        }
        offset = end;
    }
    (None, text)
}

/// One line of note body: outside the frontmatter block and outside every
/// fenced code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyLine<'a> {
    /// 1-based line number in the whole text.
    pub(crate) line: u32,
    /// Byte offset of the line's first character in the whole text.
    pub(crate) start: usize,
    /// The line without its line ending.
    pub(crate) raw: &'a str,
}

/// Every line of `text` that carries note body.
///
/// The frontmatter block, fenced code blocks and the fence lines themselves are
/// left out, so a caller scanning for links, tags or headings never has to know
/// about any of them. Inline code is per-line and is removed by
/// [`code_free_segments`].
pub(crate) fn body_lines(text: &str) -> Vec<BodyLine<'_>> {
    let (frontmatter, _) = split_frontmatter(text);
    let skip_until = frontmatter.map_or(0, str::len);

    let mut out = Vec::new();
    let mut offset = 0usize;
    let mut fence: Option<(char, usize)> = None;

    for (index, line) in text.split_inclusive('\n').enumerate() {
        let start = offset;
        offset += line.len();
        if start < skip_until {
            continue;
        }
        let raw = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = raw.trim_start();

        match fence {
            Some((marker, width)) => {
                if is_fence(trimmed, Some(marker)).is_some_and(|(_, w)| w >= width)
                    && trimmed.trim_end_matches(marker).trim().is_empty()
                {
                    fence = None;
                }
            }
            None => match is_fence(trimmed, None) {
                Some(opened) => fence = Some(opened),
                None => out.push(BodyLine {
                    line: (index as u32) + 1,
                    start,
                    raw,
                }),
            },
        }
    }
    out
}

/// The fence character and its width when `trimmed` opens or closes a fenced
/// code block, restricted to `expect` when a block is already open.
fn is_fence(trimmed: &str, expect: Option<char>) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    if expect.is_some_and(|wanted| wanted != marker) {
        return None;
    }
    let width = trimmed.chars().take_while(|c| *c == marker).count();
    (width >= 3).then_some((marker, width))
}

/// The stretches of `raw` that are not inside an inline code span, each with
/// its byte offset inside `raw`.
///
/// Backtick runs pair by width, the way CommonMark pairs them, and an unpaired
/// run is ordinary text. Spans are closed at the end of the line: a link split
/// across a multi-line code span is rare enough to lose, and the alternative is
/// carrying span state across a scan that otherwise reads one line at a time.
pub(crate) fn code_free_segments(raw: &str) -> Vec<(usize, &str)> {
    if !raw.contains('`') {
        return vec![(0, raw)];
    }

    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] != b'`' {
            index += 1;
            continue;
        }
        let open = index;
        while index < bytes.len() && bytes[index] == b'`' {
            index += 1;
        }
        let width = index - open;

        let mut probe = index;
        let close = loop {
            let Some(found) = raw[probe..].find('`') else {
                break None;
            };
            let at = probe + found;
            let mut end = at;
            while end < bytes.len() && bytes[end] == b'`' {
                end += 1;
            }
            if end - at == width {
                break Some((at, end));
            }
            probe = end;
        };

        match close {
            Some((_, end)) => {
                if open > cursor {
                    out.push((cursor, &raw[cursor..open]));
                }
                cursor = end;
                index = end;
            }
            None => break,
        }
    }

    if cursor < raw.len() {
        out.push((cursor, &raw[cursor..]));
    }
    out
}

/// Every link in `text`.
///
/// Finds `[[…]]` wikilinks and `[label](path)` markdown links whose path names
/// a note: it ends in `.md` or `.markdown`, or it carries no extension at all.
/// A URL, an image and a same-document `#anchor` are not links to a note and
/// are left out. Code fences, inline code and the frontmatter block are not
/// scanned, so a link written inside an example stays an example.
pub fn scan(text: &str) -> Vec<RawLink> {
    let mut out = Vec::new();
    for line in body_lines(text) {
        for (offset, segment) in code_free_segments(line.raw) {
            scan_segment(&line, offset, segment, &mut out);
        }
    }
    out
}

/// Scans one code-free stretch of one line.
fn scan_segment(line: &BodyLine<'_>, offset: usize, segment: &str, out: &mut Vec<RawLink>) {
    let bytes = segment.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }

        // `![alt](x)` is an image, not a link to a note.
        let is_image = index > 0 && bytes[index - 1] == b'!';

        if segment[index..].starts_with("[[") {
            match segment[index + 2..].find("]]") {
                Some(found) => {
                    let inner = &segment[index + 2..index + 2 + found];
                    let end = index + 2 + found + 2;
                    let target = parse_wikilink(inner);
                    if !target.name.is_empty() {
                        out.push(build(line, offset, index, end, LinkKind::Wikilink, target));
                    }
                    index = end;
                }
                None => break,
            }
            continue;
        }

        let Some((dest_end, dest)) = markdown_link(segment, index) else {
            index += 1;
            continue;
        };
        if !is_image {
            if let Some(target) = note_destination(dest) {
                out.push(build(
                    line,
                    offset,
                    index,
                    dest_end,
                    LinkKind::Markdown,
                    target,
                ));
            }
        }
        index = dest_end;
    }
}

/// Builds a [`RawLink`] for the link occupying `start..end` of a segment that
/// itself starts at `offset` inside `line`.
fn build(
    line: &BodyLine<'_>,
    offset: usize,
    start: usize,
    end: usize,
    kind: LinkKind,
    target: WikilinkTarget,
) -> RawLink {
    let in_line = offset + start;
    let mut written = target.name.clone();
    if let Some(folder) = &target.folder {
        written = format!("{folder}/{written}");
    }
    RawLink {
        kind,
        target: written,
        alias: target.alias,
        heading: target.heading,
        line: line.line,
        col: line.raw[..in_line].chars().count() as u32,
        byte_range: line.start + in_line..line.start + offset + end,
    }
}

/// The end offset and the destination of a `[label](dest)` starting at `open`.
fn markdown_link(segment: &str, open: usize) -> Option<(usize, &str)> {
    let bytes = segment.as_bytes();
    let mut index = open + 1;
    let mut depth = 1usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        index += 1;
    }
    if depth != 0 || index >= bytes.len() || bytes.get(index + 1) != Some(&b'(') {
        return None;
    }

    let mut cursor = index + 2;
    let start = cursor;
    let mut depth = 1usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    if depth != 0 || cursor >= bytes.len() {
        return None;
    }
    Some((cursor + 1, &segment[start..cursor]))
}

/// The target a markdown link's destination names, or `None` when it names
/// something that is not a note in the folder.
fn note_destination(dest: &str) -> Option<WikilinkTarget> {
    let dest = dest.trim();
    let dest = dest.strip_prefix('<').unwrap_or(dest);
    let dest = dest.strip_suffix('>').unwrap_or(dest);
    // A title after the destination: `[a](b.md "Title")`.
    let dest = dest.split_whitespace().next()?;
    if dest.is_empty() || dest.starts_with('#') {
        return None;
    }
    if dest.contains("://") || has_scheme(dest) {
        return None;
    }

    let (path, heading) = match dest.find('#') {
        Some(at) => (&dest[..at], Some(&dest[at + 1..])),
        None => (dest, None),
    };
    let path = percent_decode(path);
    let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
    let extension = name.rsplit_once('.').map(|(_, ext)| ext);
    match extension {
        Some(ext) if !NOTE_EXTENSIONS.iter().any(|n| n.eq_ignore_ascii_case(ext)) => return None,
        _ => {}
    }

    let mut target = parse_target(&path);
    if target.name.is_empty() {
        return None;
    }
    target.heading = heading
        .map(|h| percent_decode(h).trim().to_string())
        .filter(|h| !h.is_empty());
    Some(target)
}

/// Whether `dest` opens with a URL scheme, which makes it not a path.
fn has_scheme(dest: &str) -> bool {
    let Some(colon) = dest.find(':') else {
        return false;
    };
    if colon == 0 {
        return false;
    }
    let scheme = &dest[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `%20` and friends, the only escaping a markdown destination carries that
/// changes which file it names.
fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &text[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// Splits the inside of a `[[…]]` into its parts.
///
/// The alias is taken at the **first** `|` and the heading at the **first** `#`
/// of what is left, in that order, so `[[Note#Heading|Label]]` and
/// `[[Note|Label#not a heading]]` both read the way they look.
pub fn parse_wikilink(inner: &str) -> WikilinkTarget {
    let (left, alias) = match inner.find('|') {
        Some(at) => (&inner[..at], Some(inner[at + 1..].trim().to_string())),
        None => (inner, None),
    };
    let (path, heading) = match left.find('#') {
        Some(at) => (&left[..at], Some(left[at + 1..].trim().to_string())),
        None => (left, None),
    };

    let mut target = parse_target(path);
    target.alias = alias.filter(|a| !a.is_empty());
    target.heading = heading.filter(|h| !h.is_empty());
    target
}

/// Splits a target path — no alias, no heading — into its folder and its name.
///
/// The folder is everything before the **last** `/`, so `a/b/Note` is the note
/// `Note` in `a/b`. A note extension on the name is removed, so `[[Note.md]]`
/// and `[[Note]]` are the same link. `.` and `..` segments are dropped: a
/// folder is matched as a suffix of the candidate's own folders, which a
/// relative step cannot be part of, and dropping it makes `../ideas/Note`
/// resolve the way `ideas/Note` does.
pub fn parse_target(path: &str) -> WikilinkTarget {
    let mut parts: Vec<&str> = path
        .trim()
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .collect();
    let name = parts.pop().unwrap_or_default();
    parts.retain(|s| *s != "." && *s != "..");
    WikilinkTarget {
        name: strip_note_extension(name.trim()).to_string(),
        folder: (!parts.is_empty()).then(|| parts.join("/")),
        heading: None,
        alias: None,
    }
}

/// `name` without a trailing note extension.
fn strip_note_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if NOTE_EXTENSIONS.iter().any(|n| n.eq_ignore_ascii_case(ext)) => stem,
        _ => name,
    }
}

/// The comparison form of a note name.
///
/// NFC first, then lowercase. macOS stores a decomposed name and a link is
/// typed composed, so a note called `Café` is otherwise unreachable from the
/// link that names it; the same holds for an Arabic name copied out of one
/// application and typed into another. Both sides of every comparison in this
/// module go through here, on every platform, so the app behaves the same way
/// on all three.
pub fn name_key(text: &str) -> String {
    text.nfc().collect::<String>().to_lowercase()
}

/// The keys the file at `path` answers to when a link names it.
///
/// Its file name with a note extension removed, plus the file name as it
/// stands when the two differ. So `Note.md` answers to `note`, and a file with
/// any other extension answers to its whole name only: `[[list]]` does not
/// reach `list.txt`, `[[list.txt]]` does. Callers that group candidates by name
/// build their group keys from here, so a prefilter and [`resolve`] can never
/// disagree about what a name matches.
pub fn candidate_name_keys(path: &str) -> Vec<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
    let stem = name_key(strip_note_extension(name));
    let full = name_key(name);
    if full == stem {
        vec![stem]
    } else {
        vec![stem, full]
    }
}

/// Resolves `target` against `candidates`, as seen from the note at `from`.
///
/// A target carrying a `/` must also match the candidate's trailing folders; a
/// bare name matches on the file name alone. Comparison goes through
/// [`name_key`], so case and unicode normalisation never decide the answer.
///
/// Among the matches, the fewest path segments wins, then the deepest common
/// ancestor with `from`. **Two survivors of that ordering are
/// [`Resolution::Ambiguous`]**, not a coin flip: the candidates are handed back
/// in byte order for the user to choose between, and alphabetical order is used
/// to present them, never to pick one (ADR-034).
pub fn resolve(target: &WikilinkTarget, from: &str, candidates: &[String]) -> Resolution {
    let wanted = name_key(&target.name);
    let folder: Option<Vec<String>> = target.folder.as_ref().map(|f| {
        f.split(['/', '\\'])
            .filter(|s| !s.is_empty())
            .map(name_key)
            .collect()
    });

    let mut matched: Vec<&String> = candidates
        .iter()
        .filter(|path| {
            if !candidate_name_keys(path).contains(&wanted) {
                return false;
            }
            match &folder {
                None => true,
                Some(wanted) => folder_matches(path, wanted),
            }
        })
        .collect();
    if matched.is_empty() {
        return Resolution::Missing;
    }

    let from_segments = segments(from);
    let rank = |path: &str| {
        let count = segments(path).len();
        let shared = shared_prefix(&segments(path), &from_segments);
        (count, usize::MAX - shared)
    };
    let best = matched
        .iter()
        .map(|path| rank(path))
        .min()
        .expect("matched is not empty");
    matched.retain(|path| rank(path) == best);
    matched.sort_unstable();
    matched.dedup();

    match matched.len() {
        1 => Resolution::Resolved(matched[0].clone()),
        _ => Resolution::Ambiguous(matched.into_iter().cloned().collect()),
    }
}

/// Whether `path`'s folders end with `wanted`.
fn folder_matches(path: &str, wanted: &[String]) -> bool {
    let all = segments(path);
    let Some(folders) = all.split_last().map(|(_, rest)| rest) else {
        return wanted.is_empty();
    };
    if wanted.len() > folders.len() {
        return false;
    }
    folders[folders.len() - wanted.len()..]
        .iter()
        .map(|s| name_key(s))
        .eq(wanted.iter().cloned())
}

/// `path` split into its segments, with either separator accepted so a Windows
/// key and a posix key rank the same way.
fn segments(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|s| !s.is_empty()).collect()
}

/// How many leading segments two paths share, folded.
fn shared_prefix(left: &[&str], right: &[&str]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(a, b)| name_key(a) == name_key(b))
        .count()
}

/// A GitHub-style anchor for a heading's text.
///
/// Lowercased, everything but letters, digits, `-` and `_` dropped, spaces
/// turned into `-`. This is the form `[[Note#Heading]]` is matched against, so
/// the index and every consumer agree on what a heading is called (ADR-034).
pub fn heading_slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.nfc().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }
    out
}

/// Turns a run of slugs into the anchors a document actually has: the first of
/// a repeated slug keeps it, the second gets `-1`, the third `-2`.
pub(crate) fn disambiguate(slug: &str, taken: &mut HashSet<String>) -> String {
    if taken.insert(slug.to_string()) {
        return slug.to_string();
    }
    let mut suffix = 1u32;
    loop {
        let candidate = format!("{slug}-{suffix}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(text: &str) -> Vec<String> {
        scan(text).into_iter().map(|l| l.target).collect()
    }

    #[test]
    fn a_bare_wikilink_is_scanned() {
        let links = scan("see [[Note]] here\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Wikilink);
        assert_eq!(links[0].target, "Note");
        assert_eq!(links[0].alias, None);
        assert_eq!(links[0].heading, None);
        assert_eq!(links[0].line, 1);
        assert_eq!(links[0].col, 4);
        assert_eq!(
            &"see [[Note]] here\n"[links[0].byte_range.clone()],
            "[[Note]]"
        );
    }

    #[test]
    fn an_alias_is_split_off_the_target() {
        let links = scan("[[Note|the label]]\n");
        assert_eq!(links[0].target, "Note");
        assert_eq!(links[0].alias.as_deref(), Some("the label"));
    }

    #[test]
    fn a_heading_is_split_off_the_target() {
        let links = scan("[[Note#Some Heading]]\n");
        assert_eq!(links[0].target, "Note");
        assert_eq!(links[0].heading.as_deref(), Some("Some Heading"));
    }

    #[test]
    fn a_folder_prefix_stays_on_the_target() {
        let links = scan("[[folder/Note]]\n");
        assert_eq!(links[0].target, "folder/Note");
        assert_eq!(links[0].wikilink_target().folder.as_deref(), Some("folder"));
        assert_eq!(links[0].wikilink_target().name, "Note");
    }

    #[test]
    fn a_markdown_link_to_a_note_is_scanned() {
        let links = scan("[label](./x.md)\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Markdown);
        assert_eq!(links[0].target, "x");
    }

    #[test]
    fn a_markdown_link_with_no_extension_is_a_note_link() {
        assert_eq!(targets("[label](sub/other)\n"), vec!["sub/other"]);
    }

    #[test]
    fn a_url_an_image_and_an_anchor_are_not_note_links() {
        assert!(targets("[a](https://example.com)\n").is_empty());
        assert!(targets("[a](mailto:x@example.com)\n").is_empty());
        assert!(targets("![a](picture.png)\n").is_empty());
        assert!(targets("[a](#section)\n").is_empty());
        assert!(targets("[a](sheet.csv)\n").is_empty());
    }

    #[test]
    fn a_markdown_link_heading_becomes_the_links_heading() {
        let links = scan("[a](notes/x.md#the-part)\n");
        assert_eq!(links[0].target, "notes/x");
        assert_eq!(links[0].heading.as_deref(), Some("the-part"));
    }

    #[test]
    fn a_percent_escaped_destination_names_the_real_file() {
        assert_eq!(targets("[a](My%20Note.md)\n"), vec!["My Note"]);
    }

    #[test]
    fn a_link_inside_a_fence_is_not_scanned() {
        assert!(targets("```\n[[Note]]\n```\n").is_empty());
        assert!(targets("~~~md\n[[Note]]\n~~~\n").is_empty());
    }

    #[test]
    fn a_link_inside_inline_code_is_not_scanned() {
        assert!(targets("write `[[Note]]` to link\n").is_empty());
        assert_eq!(targets("`code` then [[Note]]\n"), vec!["Note"]);
    }

    #[test]
    fn a_link_inside_frontmatter_is_not_scanned() {
        assert!(targets("---\nsee: \"[[Note]]\"\n---\nbody\n").is_empty());
        assert_eq!(targets("---\nt: x\n---\n[[Note]]\n"), vec!["Note"]);
    }

    #[test]
    fn positions_are_one_based_lines_and_zero_based_characters() {
        let text = "first\nBefore [[Note]]\n";
        let links = scan(text);
        assert_eq!(links[0].line, 2);
        assert_eq!(links[0].col, 7);
        assert_eq!(&text[links[0].byte_range.clone()], "[[Note]]");
    }

    #[test]
    fn a_column_counts_characters_not_bytes() {
        let links = scan("مرحبا [[Note]]\n");
        assert_eq!(links[0].col, 6);
    }

    #[test]
    fn an_unterminated_wikilink_is_not_a_link() {
        assert!(targets("[[Note\n").is_empty());
    }

    #[test]
    fn parse_wikilink_takes_the_alias_first_then_the_heading() {
        let target = parse_wikilink("Note#Heading|Label");
        assert_eq!(target.name, "Note");
        assert_eq!(target.heading.as_deref(), Some("Heading"));
        assert_eq!(target.alias.as_deref(), Some("Label"));

        let target = parse_wikilink("Note|Label#not a heading");
        assert_eq!(target.name, "Note");
        assert_eq!(target.heading, None);
        assert_eq!(target.alias.as_deref(), Some("Label#not a heading"));
    }

    #[test]
    fn parse_target_splits_on_the_last_separator_and_drops_a_note_extension() {
        let target = parse_target("a/b/Note.md");
        assert_eq!(target.folder.as_deref(), Some("a/b"));
        assert_eq!(target.name, "Note");
        assert_eq!(parse_target("Note.txt").name, "Note.txt");
    }

    #[test]
    fn a_missing_target_resolves_to_missing() {
        let candidates = vec!["/n/Other.md".to_string()];
        let resolution = resolve(&parse_target("Note"), "/n/From.md", &candidates);
        assert_eq!(resolution, Resolution::Missing);
    }

    #[test]
    fn the_shallower_of_two_notes_with_one_name_wins() {
        let candidates = vec![
            "/n/deep/inner/Note.md".to_string(),
            "/n/Note.md".to_string(),
        ];
        assert_eq!(
            resolve(&parse_target("Note"), "/n/deep/From.md", &candidates),
            Resolution::Resolved("/n/Note.md".to_string())
        );
    }

    #[test]
    fn equal_depth_is_broken_by_the_nearer_common_ancestor() {
        let candidates = vec!["/n/a/Note.md".to_string(), "/n/b/Note.md".to_string()];
        assert_eq!(
            resolve(&parse_target("Note"), "/n/b/From.md", &candidates),
            Resolution::Resolved("/n/b/Note.md".to_string())
        );
    }

    #[test]
    fn a_genuine_tie_is_ambiguous_and_is_never_guessed() {
        let candidates = vec!["/n/a/Note.md".to_string(), "/n/b/Note.md".to_string()];
        assert_eq!(
            resolve(&parse_target("Note"), "/n/c/From.md", &candidates),
            Resolution::Ambiguous(vec!["/n/a/Note.md".to_string(), "/n/b/Note.md".to_string()])
        );
    }

    #[test]
    fn a_folder_prefix_picks_between_two_notes_of_one_name() {
        let candidates = vec!["/n/a/Note.md".to_string(), "/n/b/Note.md".to_string()];
        assert_eq!(
            resolve(&parse_target("b/Note"), "/n/c/From.md", &candidates),
            Resolution::Resolved("/n/b/Note.md".to_string())
        );
    }

    #[test]
    fn case_never_decides_a_resolution() {
        let candidates = vec!["/n/Weekly Review.md".to_string()];
        assert_eq!(
            resolve(&parse_target("weekly review"), "/n/From.md", &candidates),
            Resolution::Resolved("/n/Weekly Review.md".to_string())
        );
    }

    #[test]
    fn a_decomposed_name_resolves_from_a_composed_link() {
        // "Café" written NFD, the spelling a macOS filesystem hands back.
        let candidates = vec!["/n/Cafe\u{301}.md".to_string()];
        assert_eq!(
            resolve(&parse_target("Café"), "/n/From.md", &candidates),
            Resolution::Resolved("/n/Cafe\u{301}.md".to_string())
        );
    }

    #[test]
    fn an_arabic_name_resolves() {
        let candidates = vec!["/n/ملاحظات.md".to_string()];
        assert_eq!(
            resolve(&parse_target("ملاحظات"), "/n/From.md", &candidates),
            Resolution::Resolved("/n/ملاحظات.md".to_string())
        );
    }

    #[test]
    fn a_full_file_name_with_another_extension_still_resolves() {
        let candidates = vec!["/n/list.txt".to_string()];
        assert_eq!(
            resolve(&parse_target("list.txt"), "/n/From.md", &candidates),
            Resolution::Resolved("/n/list.txt".to_string())
        );
    }

    #[test]
    fn candidate_name_keys_cover_the_stem_and_the_whole_name() {
        assert_eq!(candidate_name_keys("/n/Note.md"), vec!["note", "note.md"]);
        assert_eq!(candidate_name_keys("/n/list.txt"), vec!["list.txt"]);
    }

    #[test]
    fn split_frontmatter_hands_back_the_block_and_the_body() {
        let (block, body) = split_frontmatter("---\na: 1\n---\nbody\n");
        assert_eq!(block, Some("---\na: 1\n---\n"));
        assert_eq!(body, "body\n");
    }

    #[test]
    fn an_unterminated_block_is_body_text() {
        let text = "---\na: 1\nstill body\n";
        assert_eq!(split_frontmatter(text), (None, text));
    }

    #[test]
    fn heading_slug_matches_the_github_shape() {
        assert_eq!(heading_slug("Some Heading"), "some-heading");
        assert_eq!(heading_slug("What's next?"), "whats-next");
        assert_eq!(heading_slug("Café أهلا"), "café-أهلا");
    }

    #[test]
    fn a_repeated_slug_gets_a_numeric_suffix_from_the_second_one() {
        let mut taken = HashSet::new();
        assert_eq!(disambiguate("notes", &mut taken), "notes");
        assert_eq!(disambiguate("notes", &mut taken), "notes-1");
        assert_eq!(disambiguate("notes", &mut taken), "notes-2");
    }

    #[test]
    fn code_free_segments_drop_paired_backtick_runs_only() {
        assert_eq!(code_free_segments("a `b` c"), vec![(0, "a "), (5, " c")]);
        assert_eq!(code_free_segments("a ` b c"), vec![(0, "a ` b c")]);
    }

    #[test]
    fn body_lines_skip_the_frontmatter_and_every_fence() {
        let text = "---\na: 1\n---\nbody\n```\nfenced\n```\ntail\n";
        let lines: Vec<_> = body_lines(text).into_iter().map(|l| l.raw).collect();
        assert_eq!(lines, vec!["body", "tail"]);
    }
}
