//! What a note says about itself: its links, its frontmatter properties, its
//! tags and its headings.
//!
//! These are the four derived tables the notes index keeps beside `files`
//! (migration 040). Deriving them is pure and lives here so the index, the
//! preview and the command line all read a note the same way; storing them is
//! `writ_storage::notes_index`.
//!
//! Nothing is dropped on the way through. A frontmatter value this module
//! cannot read as a scalar or a flat sequence is kept as the JSON string of
//! what was written, because a property the user can see in their editor and
//! cannot see in Writ is worse than an ugly one.

use std::collections::HashSet;

use serde_json::Value;

use super::links::{
    body_lines, code_free_segments, disambiguate, heading_slug, scan, split_frontmatter, RawLink,
};

/// One heading in a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// `1` for `#` through `6` for `######`.
    pub level: u8,
    /// The heading text, with the `#` markers and any closing run removed.
    pub text: String,
    /// 1-based line the heading is on.
    pub line: u32,
    /// The anchor `[[Note#Heading]]` matches, unique inside the note.
    pub slug: String,
}

/// Everything one note says about itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteFacts {
    /// Links out of the note, in the order they appear.
    pub links: Vec<RawLink>,
    /// Frontmatter properties, in the order they are written.
    pub properties: Vec<(String, Value)>,
    /// Each `#tag` and the 1-based line it is on.
    pub tags: Vec<(String, u32)>,
    /// Headings, in document order.
    pub headings: Vec<Heading>,
}

/// Reads every fact out of `text`.
pub fn extract(text: &str) -> NoteFacts {
    NoteFacts {
        links: scan(text),
        properties: properties(text),
        tags: tags(text),
        headings: headings(text),
    }
}

/// The frontmatter properties of `text`.
///
/// Flat YAML only: `key: scalar`, `key: [a, b]`, a `key:` followed by `-`
/// items, and a `key: |` or `key: >` block scalar, whose text is the value. A
/// key whose value is a nested mapping keeps the block it was written as, as a
/// JSON string, so nothing the user wrote disappears from the index.
pub fn properties(text: &str) -> Vec<(String, Value)> {
    let (Some(block), _) = split_frontmatter(text) else {
        return Vec::new();
    };
    // The block always opens and closes with a `---` line.
    let inner: Vec<&str> = block
        .lines()
        .skip(1)
        .take_while(|line| line.trim_end() != "---")
        .collect();

    let mut out = Vec::new();
    let mut index = 0usize;
    while index < inner.len() {
        let line = inner[index];
        index += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || line.starts_with([' ', '\t', '-']) {
            continue;
        }
        let Some((key, rest)) = split_key(trimmed) else {
            continue;
        };
        let rest = rest.trim();
        if let Some(marker) = block_scalar(rest) {
            let start = index;
            while index < inner.len()
                && (inner[index].trim().is_empty() || inner[index].starts_with([' ', '\t']))
            {
                index += 1;
            }
            // Blank lines after the last indented one close the block; they
            // belong to whatever comes next.
            while index > start && inner[index - 1].trim().is_empty() {
                index -= 1;
            }
            out.push((key.to_string(), block_text(marker, &inner[start..index])));
            continue;
        }
        if !rest.is_empty() {
            out.push((key.to_string(), scalar(rest)));
            continue;
        }

        let start = index;
        while index < inner.len() && inner[index].starts_with([' ', '\t', '-']) {
            index += 1;
        }
        let block = &inner[start..index];
        let value = if block.is_empty() {
            Value::Null
        } else if block.iter().all(|l| l.trim_start().starts_with("- ")) {
            Value::Array(
                block
                    .iter()
                    .map(|l| scalar(l.trim_start().trim_start_matches("- ").trim()))
                    .collect(),
            )
        } else {
            // Nested, or something this parser does not model. Kept whole.
            Value::String(block.join("\n"))
        };
        out.push((key.to_string(), value));
    }
    out
}

/// The `|` or `>` of a block scalar header, or `None` when `rest` is an
/// ordinary value.
///
/// A chomping or indentation indicator may follow the marker: `|-`, `>+`, `|2`.
fn block_scalar(rest: &str) -> Option<char> {
    let mut chars = rest.chars();
    let marker = chars.next()?;
    if marker != '|' && marker != '>' {
        return None;
    }
    chars
        .all(|c| matches!(c, '-' | '+') || c.is_ascii_digit())
        .then_some(marker)
}

/// The text of a block scalar: `|` keeps its line breaks, `>` folds each
/// paragraph onto one line.
///
/// The common indentation is stripped, which is the part of YAML's rule that
/// changes the text a reader sees. Chomping indicators are not modelled; a
/// trailing blank line is not worth a second parser.
///
/// The indent is counted and dropped in **characters**. A line's leading
/// whitespace can be a non-breaking space, which is two bytes and is what a
/// paste from a web page leaves behind, so a byte count taken from one line
/// lands inside a character of another.
fn block_text(marker: char, block: &[&str]) -> Value {
    let indent = block
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| indent_chars(line))
        .min()
        .unwrap_or(0);
    let lines = block.iter().map(|line| drop_indent(line, indent));

    if marker == '|' {
        return Value::String(lines.collect::<Vec<_>>().join("\n"));
    }
    let mut folded = String::new();
    for line in lines {
        if line.trim().is_empty() {
            folded.push('\n');
            continue;
        }
        if !folded.is_empty() && !folded.ends_with('\n') {
            folded.push(' ');
        }
        folded.push_str(line);
    }
    Value::String(folded)
}

/// How many characters of leading whitespace `line` carries.
fn indent_chars(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// `line` without `indent` characters of leading whitespace, or without all of
/// it when the line is shorter than that.
fn drop_indent(line: &str, indent: usize) -> &str {
    let mut chars = line.chars();
    for _ in 0..indent {
        if !chars.next().is_some_and(char::is_whitespace) {
            return line.trim_start();
        }
    }
    chars.as_str()
}

/// The key and the rest of a `key: value` line, or `None` when the line has no
/// key at all.
fn split_key(line: &str) -> Option<(&str, &str)> {
    let (key, rest) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(['[', ']', '{', '}']) {
        return None;
    }
    Some((key.trim_matches(['"', '\'']), rest))
}

/// One YAML scalar, or a `[a, b]` flow sequence of them.
fn scalar(raw: &str) -> Value {
    let raw = raw.trim();
    if raw.is_empty() || raw == "~" || raw.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Some(inner) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return Value::Array(
            split_flow(inner)
                .into_iter()
                .filter(|item| !item.trim().is_empty())
                .map(scalar)
                .collect(),
        );
    }
    if raw.len() >= 2 {
        for quote in ['"', '\''] {
            if raw.starts_with(quote) && raw.ends_with(quote) {
                return Value::String(raw[1..raw.len() - 1].to_string());
            }
        }
    }
    if raw == "true" || raw == "false" {
        return Value::Bool(raw == "true");
    }
    if let Ok(number) = raw.parse::<i64>() {
        return Value::from(number);
    }
    if let Ok(number) = raw.parse::<f64>() {
        if let Some(value) = serde_json::Number::from_f64(number) {
            return Value::Number(value);
        }
    }
    Value::String(raw.to_string())
}

/// Splits a flow sequence's body on the commas that are not inside a quote.
fn split_flow(inner: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    for (at, ch) in inner.char_indices() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == ',' => {
                out.push(&inner[start..at]);
                start = at + 1;
            }
            None => {}
        }
    }
    out.push(&inner[start..]);
    out
}

/// Every tag in `text`, with the line it is on: the ones written under a
/// frontmatter `tags` key first, then the `#tags` written in the body.
///
/// A body tag opens at the start of a line or after whitespace or an opening
/// bracket, which is what keeps the fragment of `https://example.com#section`
/// out of the list, and runs over letters, digits, `_`, `-` and `/`. A run of
/// digits alone is a number, not a tag. Code fences and inline code are not
/// scanned, so a tag written inside an example stays an example.
///
/// A frontmatter tag is a tag the same way an inline one is: it is what the
/// note is filed under, and a folder written in another editor puts most of
/// its tags there ([`frontmatter_tags`]).
///
/// Every tag comes back lowercased. `#Project` and `#project` are the same
/// tag to the person who wrote them, and two rows in the tag list is two
/// halves of one pile. The note keeps the casing it was written with; only
/// what the tag is filed under is folded.
pub fn tags(text: &str) -> Vec<(String, u32)> {
    let mut out = frontmatter_tags(text);
    for line in body_lines(text) {
        for (offset, segment) in code_free_segments(line.raw) {
            let mut chars = segment.char_indices().peekable();
            while let Some((at, ch)) = chars.next() {
                if ch != '#' || !opens_a_tag(&line.raw[..offset + at]) {
                    continue;
                }
                let body: String = segment[at..]
                    .chars()
                    .skip(1)
                    .take_while(|c| is_tag_char(*c))
                    .collect();
                if is_tag(&body) {
                    out.push((body.to_lowercase(), line.line));
                    for _ in 0..body.chars().count() {
                        chars.next();
                    }
                }
            }
        }
    }
    out
}

/// The frontmatter keys a note's tags can be written under.
///
/// `tags` is the key Obsidian writes; `tag` is the singular spelling older
/// notes carry. No other key is read as a tag: a `topics` list holds
/// properties, and reading it as tags would file notes under words nobody
/// tagged them with.
const TAG_KEYS: &[&str] = &["tags", "tag"];

/// Every tag written in the frontmatter of `text`, with the line it is on.
///
/// `tags: [a, b]` puts both tags on the line the key is written on, and a `-`
/// list under `tags:` puts each tag on the line its item is written on, so a
/// reader can be taken to the tag it clicked either way. Quotes and a leading
/// `#` come off the value, and what is left has to be a tag [`is_tag`] answers
/// for, so `tags: [2026]` and an empty entry add nothing.
///
/// A value written after the key holds as many tags as it names:
/// `tags: work, urgent`, `tags: "work, urgent"` and `tags: work urgent` are
/// all two tags, which is what the folders people arrive with carry. Inside a
/// list the item is the tag, so `[two words]` is not one ([`TagValue`]).
///
/// The block is read line by line rather than through [`properties`]: the
/// properties parser collects the block first and keeps no line numbers, and a
/// tag without its line is a tag nothing can jump to.
fn frontmatter_tags(text: &str) -> Vec<(String, u32)> {
    let (Some(block), _) = split_frontmatter(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut in_tag_list = false;
    for (index, line) in block.lines().enumerate().skip(1) {
        let number = index as u32 + 1;
        if line.trim_end() == "---" {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix('-') {
            if in_tag_list {
                push_tags(&mut out, without_comment(item), TagValue::Item, number);
            }
            continue;
        }
        // An indented line belongs to the value above it, which a nested map
        // makes a block of its own keys. A `tags` key inside one is that map's,
        // not the note's.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, rest)) = split_key(trimmed) else {
            in_tag_list = false;
            continue;
        };
        if !TAG_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
            in_tag_list = false;
            continue;
        }
        // The comment comes off before the value is read, so a note the writer
        // left themselves neither becomes a tag nor takes the line's tags with
        // it.
        let rest = without_comment(rest).trim();
        // A key with nothing after it opens the list the `-` items below it
        // belong to.
        in_tag_list = rest.is_empty();
        if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            for item in split_flow(inner) {
                push_tags(&mut out, item, TagValue::Listed, number);
            }
        } else if !rest.is_empty() {
            push_tags(&mut out, rest, TagValue::AfterTheKey, number);
        }
    }
    out
}

/// Where a frontmatter tag value is written, which is what decides how many
/// tags one value can hold.
enum TagValue {
    /// Written after the key. A comma and a space both separate two tags:
    /// `tags: work, urgent` and `tags: work urgent` are two tags each.
    AfterTheKey,
    /// One `-` item. A comma still separates two tags; a space does not,
    /// because the item is already a value of its own.
    Item,
    /// One item of a `[a, b]` list, which the list has already separated.
    Listed,
}

/// Records every tag written in `raw` on `number`, dropping what is not one.
///
/// A piece is a tag when it is made of tag characters and [`is_tag`] answers
/// for it, so a stray separator, an empty entry and `2026` add nothing while
/// the pieces beside them are still kept.
fn push_tags(out: &mut Vec<(String, u32)>, raw: &str, written: TagValue, number: u32) {
    let value = raw.trim().trim_matches(['"', '\'']).trim();
    let pieces: Vec<&str> = match written {
        TagValue::AfterTheKey => value.split([',', ' ', '\t']).collect(),
        TagValue::Item => value.split(',').collect(),
        TagValue::Listed => vec![value],
    };
    for piece in pieces {
        let piece = piece.trim().trim_matches(['"', '\'']).trim();
        let body = piece.strip_prefix('#').unwrap_or(piece);
        if body.chars().all(is_tag_char) && is_tag(body) {
            out.push((body.to_lowercase(), number));
        }
    }
}

/// `value` with the comment a writer left on the line removed.
///
/// A `#` with nothing tag-shaped after it opens a YAML comment; a `#` in front
/// of a tag character marks a tag. So `tags: work # mine later` is one tag and
/// a note to the writer, `tags: #work #mine` is two tags, and
/// `tags: [a, b] # mine` keeps its list.
fn without_comment(value: &str) -> &str {
    let mut from = 0;
    while let Some(offset) = value[from..].find('#') {
        let at = from + offset;
        if value[at + 1..].chars().next().is_some_and(is_tag_char) {
            from = at + 1;
            continue;
        }
        return &value[..at];
    }
    value
}

/// Whether a `#` written after `before` can open a tag.
///
/// The character in front of it decides, with one exception: `](#anchor)` is a
/// markdown link to a heading in this note, never a tag. Reading those as tags
/// puts the section names of every note that carries a table of contents into
/// the tag list.
fn opens_a_tag(before: &str) -> bool {
    let mut back = before.chars().rev();
    let Some(previous) = back.next() else {
        return true;
    };
    if previous == '(' {
        return back.next() != Some(']');
    }
    previous.is_whitespace() || matches!(previous, '[' | '{' | '>' | '"' | '\'')
}

/// Whether `ch` can appear in a tag.
fn is_tag_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/')
}

/// Whether `body` is a tag rather than a number, a bare separator or nothing.
fn is_tag(body: &str) -> bool {
    !body.is_empty() && body.chars().any(|c| c.is_alphabetic() || c == '_')
}

/// Every ATX heading in the body of `text`, each with the anchor a link points
/// at.
///
/// A repeated heading text gets the GitHub disambiguator: the first keeps the
/// bare slug, the second gets `-1`, the third `-2`.
pub fn headings(text: &str) -> Vec<Heading> {
    let mut taken = HashSet::new();
    let mut out = Vec::new();
    for line in body_lines(text) {
        let trimmed = line.raw.trim_start();
        if line.raw.len() - trimmed.len() > 3 || !trimmed.starts_with('#') {
            continue;
        }
        let rest = trimmed.trim_start_matches('#');
        let level = trimmed.len() - rest.len();
        if !(1..=6).contains(&level) {
            continue;
        }
        if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
            continue;
        }
        let heading = rest.trim().trim_end_matches('#').trim();
        out.push(Heading {
            level: level as u8,
            text: heading.to_string(),
            line: line.line,
            slug: disambiguate(&heading_slug(heading), &mut taken),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_scalars_and_sequences_are_read() {
        let facts = extract("---\ntitle: Weekly review\ndone: true\ncount: 3\ntags: [a, b]\n---\n");
        assert_eq!(
            facts.properties,
            vec![
                ("title".to_string(), Value::from("Weekly review")),
                ("done".to_string(), Value::Bool(true)),
                ("count".to_string(), Value::from(3)),
                (
                    "tags".to_string(),
                    Value::Array(vec![Value::from("a"), Value::from("b")])
                ),
            ]
        );
    }

    #[test]
    fn a_block_sequence_is_read() {
        let props = properties("---\ntags:\n  - one\n  - two\n---\n");
        assert_eq!(
            props,
            vec![(
                "tags".to_string(),
                Value::Array(vec![Value::from("one"), Value::from("two")])
            )]
        );
    }

    #[test]
    fn a_quoted_value_keeps_its_text_and_loses_its_quotes() {
        let props = properties("---\ntitle: \"a: b\"\n---\n");
        assert_eq!(props, vec![("title".to_string(), Value::from("a: b"))]);
    }

    #[test]
    fn a_nested_mapping_is_kept_as_written_rather_than_dropped() {
        let props = properties("---\nmeta:\n  a: 1\n  b: 2\n---\n");
        assert_eq!(
            props,
            vec![("meta".to_string(), Value::from("  a: 1\n  b: 2"))]
        );
    }

    #[test]
    fn an_empty_value_is_null() {
        assert_eq!(
            properties("---\nnote:\n---\n"),
            vec![("note".to_string(), Value::Null)]
        );
    }

    #[test]
    fn a_note_without_frontmatter_has_no_properties() {
        assert!(properties("# Heading\n\nbody\n").is_empty());
    }

    #[test]
    fn a_tag_is_found_at_the_start_of_a_line_and_after_a_space() {
        assert_eq!(
            tags("#inbox and some #draft/two text\n"),
            vec![("inbox".to_string(), 1), ("draft/two".to_string(), 1)]
        );
    }

    #[test]
    fn a_url_fragment_is_not_a_tag() {
        assert!(tags("see https://example.com/x#section for more\n").is_empty());
    }

    #[test]
    fn a_number_and_a_heading_marker_are_not_tags() {
        assert!(tags("issue #123 and\n").is_empty());
        assert!(tags("# Heading\n").is_empty());
        assert!(tags("## Sub\n").is_empty());
    }

    #[test]
    fn a_tag_inside_code_is_not_scanned() {
        assert!(tags("```\n#reading\n```\n").is_empty());
        assert!(tags("write `#reading` for that\n").is_empty());
    }

    #[test]
    fn a_hash_in_the_frontmatter_block_is_not_an_inline_tag() {
        assert!(tags("---\ntitle: x\n#reading: y\n---\nbody\n").is_empty());
    }

    #[test]
    fn a_frontmatter_tag_list_carries_the_line_the_key_is_written_on() {
        assert_eq!(
            tags("---\ntitle: One\ntags: [alpha, project/beta]\n---\n\n#gamma\n"),
            vec![
                ("alpha".to_string(), 3),
                ("project/beta".to_string(), 3),
                ("gamma".to_string(), 6),
            ]
        );
    }

    #[test]
    fn a_frontmatter_tag_written_as_an_item_carries_its_own_line() {
        assert_eq!(
            tags("---\ntag: alpha\ntags:\n  - beta\n  - \"#project/gamma\"\n---\nbody\n"),
            vec![
                ("alpha".to_string(), 2),
                ("beta".to_string(), 4),
                ("project/gamma".to_string(), 5),
            ]
        );
    }

    #[test]
    fn a_frontmatter_value_that_is_not_a_tag_is_left_out() {
        assert!(tags("---\ntags: [2026, \"\"]\ntopics: [alpha]\n---\nbody\n").is_empty());
        assert!(tags("---\ncover:\n  tags: [alpha]\n---\nbody\n").is_empty());
        assert!(tags("---\ntags: [two words]\n---\nbody\n").is_empty());
    }

    #[test]
    fn a_frontmatter_tag_value_holds_as_many_tags_as_it_names() {
        assert_eq!(
            tags("---\ntags: work, urgent\n---\nbody\n"),
            vec![("work".to_string(), 2), ("urgent".to_string(), 2)]
        );
        assert_eq!(
            tags("---\ntags: \"work, urgent\"\n---\nbody\n"),
            vec![("work".to_string(), 2), ("urgent".to_string(), 2)]
        );
        assert_eq!(
            tags("---\ntags: work urgent\n---\nbody\n"),
            vec![("work".to_string(), 2), ("urgent".to_string(), 2)]
        );
        assert_eq!(
            tags("---\ntag: work, urgent\n---\nbody\n"),
            vec![("work".to_string(), 2), ("urgent".to_string(), 2)],
            "the older singular key holds a list the same way"
        );
        assert_eq!(
            tags("---\ntags:\n  - work, urgent\n---\nbody\n"),
            vec![("work".to_string(), 3), ("urgent".to_string(), 3)],
            "and so does one item of a list"
        );
    }

    #[test]
    fn a_comment_on_a_tag_line_is_not_a_tag_and_takes_none_with_it() {
        assert_eq!(
            tags("---\ntags: [alpha, beta] # sort these\n---\nbody\n"),
            vec![("alpha".to_string(), 2), ("beta".to_string(), 2)]
        );
        assert_eq!(
            tags("---\ntags: work # mine later\n---\nbody\n"),
            vec![("work".to_string(), 2)]
        );
        assert_eq!(
            tags("---\ntags: #work #urgent\n---\nbody\n"),
            vec![("work".to_string(), 2), ("urgent".to_string(), 2)],
            "a hash in front of a tag character marks a tag rather than a comment"
        );
        assert_eq!(
            tags("---\ntags: # the ones below\n  - work\n---\nbody\n"),
            vec![("work".to_string(), 3)],
            "a commented key still opens the list under it"
        );
    }

    #[test]
    fn one_tag_written_two_ways_is_one_tag() {
        assert_eq!(
            tags("---\ntags: [Project]\n---\n\n#project and #PROJECT\n"),
            vec![
                ("project".to_string(), 2),
                ("project".to_string(), 5),
                ("project".to_string(), 5),
            ]
        );
    }

    #[test]
    fn headings_carry_their_level_line_and_slug() {
        let found = headings("# One\n\ntext\n\n### Two Words\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].level, 1);
        assert_eq!(found[0].text, "One");
        assert_eq!(found[0].line, 1);
        assert_eq!(found[0].slug, "one");
        assert_eq!(found[1].level, 3);
        assert_eq!(found[1].line, 5);
        assert_eq!(found[1].slug, "two-words");
    }

    #[test]
    fn a_repeated_heading_gets_a_disambiguated_slug() {
        let found = headings("# Notes\n# Notes\n# Notes\n");
        let slugs: Vec<&str> = found.iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(slugs, vec!["notes", "notes-1", "notes-2"]);
    }

    #[test]
    fn a_closing_hash_run_is_not_part_of_the_heading_text() {
        assert_eq!(headings("## Title ##\n")[0].text, "Title");
    }

    #[test]
    fn a_hash_inside_a_fence_is_not_a_heading() {
        assert!(headings("```\n# Not a heading\n```\n").is_empty());
    }

    #[test]
    fn extract_reads_all_four_kinds_of_fact_from_one_note() {
        let facts =
            extract("---\ntitle: Test\n---\n# Heading\n\n#inbox see [[Other]] and [x](y.md)\n");
        assert_eq!(facts.properties.len(), 1);
        assert_eq!(facts.headings.len(), 1);
        assert_eq!(facts.tags, vec![("inbox".to_string(), 6)]);
        assert_eq!(facts.links.len(), 2);
        assert_eq!(facts.links[0].target, "Other");
        assert_eq!(facts.links[1].target, "y");
    }
}
