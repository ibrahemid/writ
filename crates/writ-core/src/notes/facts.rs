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
/// Flat YAML only: `key: scalar`, `key: [a, b]`, and a `key:` followed by `-`
/// items. A key whose value is a nested mapping keeps the block it was written
/// as, as a JSON string, so nothing the user wrote disappears from the index.
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

/// The key and the rest of a `key: value` line, or `None` when the line has no
/// key at all.
fn split_key(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    if key.is_empty() || key.contains(['[', ']', '{', '}']) {
        return None;
    }
    Some((key.trim_matches(['"', '\'']), &line[colon + 1..]))
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

/// Every `#tag` in the body of `text`, with the line it is on.
///
/// A tag opens at the start of a line or after whitespace or an opening
/// bracket, which is what keeps the fragment of `https://example.com#section`
/// out of the list, and runs over letters, digits, `_`, `-` and `/`. A run of
/// digits alone is a number, not a tag. Code fences, inline code and the
/// frontmatter block are not scanned.
pub fn tags(text: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for line in body_lines(text) {
        for (offset, segment) in code_free_segments(line.raw) {
            let mut chars = segment.char_indices().peekable();
            let mut previous: Option<char> = if offset == 0 {
                None
            } else {
                line.raw[..offset].chars().next_back()
            };
            while let Some((at, ch)) = chars.next() {
                if ch == '#' && opens_a_tag(previous) {
                    let rest = &segment[at + 1..];
                    let body: String = rest.chars().take_while(|c| is_tag_char(*c)).collect();
                    if is_tag(&body) {
                        out.push((body.clone(), line.line));
                        for _ in 0..body.chars().count() {
                            chars.next();
                        }
                        previous = body.chars().next_back();
                        continue;
                    }
                }
                previous = Some(ch);
            }
        }
    }
    out
}

/// Whether a `#` preceded by `previous` can open a tag.
fn opens_a_tag(previous: Option<char>) -> bool {
    match previous {
        None => true,
        Some(ch) => ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '>' | '"' | '\''),
    }
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
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if !(1..=6).contains(&level) {
            continue;
        }
        let rest = &trimmed[level..];
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
    fn a_tag_inside_code_or_frontmatter_is_not_scanned() {
        assert!(tags("```\n#inbox\n```\n").is_empty());
        assert!(tags("write `#inbox` for that\n").is_empty());
        assert!(tags("---\ntags: [x]\n#inbox: y\n---\nbody\n").is_empty());
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
