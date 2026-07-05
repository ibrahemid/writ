//! Full-text search query policy.
//!
//! User input from the sidebar search box is raw text. It must never reach
//! the FTS5 `MATCH` parser verbatim: a stray `"`, `*`, `-`, `NEAR`, or column
//! filter (`title:`) either errors the query or silently changes its meaning.
//! This module converts a raw query into a safe prefix-match expression so
//! that typing `tok` finds `token` and `tokenize` (search-as-you-type), backed
//! by the prefix index from migration 030.
//!
//! Beyond query construction, this module also turns a matched buffer into a
//! display [`SearchHit`]: the line number of the first matching content line
//! and a highlighted snippet built from [`SnippetSegment`]s. Highlighting is
//! literal and ASCII-case-insensitive; the FTS index folds diacritics, so a
//! diacritic-only difference (e.g. `cafe` matching `café`) is found but not
//! visually highlighted. Snippets are returned as pre-split segments rather
//! than embedded markup so the renderer never has to parse HTML.

use serde::{Deserialize, Serialize};

/// A run of snippet text, flagged whether it is part of a search match. The
/// frontend renders `matched` runs highlighted and the rest plain, with no
/// index arithmetic and no markup parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetSegment {
    /// The run of text.
    pub text: String,
    /// Whether this run is part of a search match (rendered highlighted).
    pub matched: bool,
}

/// A single search result ready for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Id of the matched buffer.
    pub buffer_id: String,
    /// Buffer title (shown as the result's file label).
    pub title: String,
    /// 1-based line number of the matching content line, or `None` when the
    /// match was on the title or no content line matched.
    pub line: Option<u32>,
    /// Highlighted preview of the matching (or first) line.
    pub snippet: Vec<SnippetSegment>,
}

/// Longest snippet rendered, in characters; longer lines are windowed.
const SNIPPET_MAX_CHARS: usize = 120;
/// When windowing a long line, how many characters to keep before the match.
const SNIPPET_LEAD_CHARS: usize = 24;

/// Returns the lowercased search terms (each at least [`MIN_TOKEN_LEN`] long)
/// used for snippet highlighting. Reuses the query tokenizer so highlighted
/// terms are exactly the terms the FTS `MATCH` expression searched for.
pub fn search_terms(raw: &str) -> Vec<String> {
    tokenize(raw)
}

/// Builds a display [`SearchHit`] for a matched buffer from its full content
/// and the search terms.
///
/// Prefers the first content line that contains a term (with its 1-based line
/// number); falls back to the first non-empty content line, then to the title,
/// each with `line` `None`. The snippet is always derived from whichever text
/// is chosen, windowed to [`SNIPPET_MAX_CHARS`].
pub fn build_hit(buffer_id: &str, title: &str, content: &str, terms: &[String]) -> SearchHit {
    for (idx, line) in content.lines().enumerate() {
        if line_contains_term(line, terms) {
            return SearchHit {
                buffer_id: buffer_id.to_string(),
                title: title.to_string(),
                line: Some((idx as u32) + 1),
                snippet: highlight_window(line, terms),
            };
        }
    }

    let fallback = content.lines().find(|l| !l.trim().is_empty());
    let snippet_source = fallback.unwrap_or(title);
    SearchHit {
        buffer_id: buffer_id.to_string(),
        title: title.to_string(),
        line: None,
        snippet: highlight_window(snippet_source, terms),
    }
}

fn line_contains_term(line: &str, terms: &[String]) -> bool {
    let lower = line.to_lowercase();
    terms.iter().any(|t| lower.contains(t.as_str()))
}

/// Returns merged, ascending match spans (char indices) of any term in `chars`,
/// matched ASCII-case-insensitively.
fn match_spans(chars: &[char], terms: &[String]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for term in terms {
        let tchars: Vec<char> = term.chars().collect();
        if tchars.is_empty() {
            continue;
        }
        let mut i = 0;
        while i + tchars.len() <= chars.len() {
            let hit = chars[i..i + tchars.len()]
                .iter()
                .zip(&tchars)
                .all(|(c, t)| c.eq_ignore_ascii_case(t));
            if hit {
                spans.push((i, i + tchars.len()));
                i += tchars.len();
            } else {
                i += 1;
            }
        }
    }
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in spans {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    merged
}

fn highlight_window(text: &str, terms: &[String]) -> Vec<SnippetSegment> {
    let chars: Vec<char> = text.chars().collect();
    let spans = match_spans(&chars, terms);

    let mut start = 0usize;
    let mut lead = false;
    if let Some(&(first, _)) = spans.first() {
        if chars.len() > SNIPPET_MAX_CHARS && first > SNIPPET_LEAD_CHARS {
            start = first - SNIPPET_LEAD_CHARS;
            lead = true;
        }
    }
    if !lead {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }

    let end = (start + SNIPPET_MAX_CHARS).min(chars.len());
    let tail = end < chars.len();

    let mut out = Vec::new();
    if lead {
        out.push(ellipsis());
    }
    let mut pos = start;
    for &(s, e) in &spans {
        let s = s.max(start);
        let e = e.min(end);
        if s >= e {
            continue;
        }
        if s > pos {
            out.push(segment(&chars[pos..s], false));
        }
        out.push(segment(&chars[s..e], true));
        pos = e;
    }
    if pos < end {
        out.push(segment(&chars[pos..end], false));
    }
    if tail {
        out.push(ellipsis());
    }
    out
}

fn segment(chars: &[char], matched: bool) -> SnippetSegment {
    SnippetSegment {
        text: chars.iter().collect(),
        matched,
    }
}

fn ellipsis() -> SnippetSegment {
    SnippetSegment {
        text: "…".to_string(),
        matched: false,
    }
}

/// Minimum length of a usable search token. Single characters produce a
/// prefix scan that matches almost every buffer; below this the query is
/// treated as empty and yields no results, which is also the server-side
/// minimum-query-length guard.
pub const MIN_TOKEN_LEN: usize = 2;

/// Splits raw search input into alphanumeric tokens.
///
/// Tokenization mirrors the FTS5 `unicode61` tokenizer closely enough for
/// query construction: runs of alphanumeric characters are tokens, every
/// other character is a separator. This strips all FTS5 syntax, so the
/// result can never be a query operator or an unbalanced quote.
fn tokenize(raw: &str) -> Vec<String> {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= MIN_TOKEN_LEN)
        .map(|t| t.to_lowercase())
        .collect()
}

/// Builds a prefix `MATCH` expression from raw user input, or `None` when no
/// token survives the minimum-length filter (empty query, punctuation only,
/// or only one-character fragments).
///
/// Each surviving token becomes a quoted prefix term — `"tok"*` — which is
/// valid FTS5 (a string token followed by `*`) and immune to operator
/// injection because the token text is wrapped in double quotes with any
/// embedded quote doubled. Multiple tokens are joined by space (implicit AND),
/// so `edit buf` requires both prefixes to hit.
pub fn to_prefix_match(raw: &str) -> Option<String> {
    let tokens = tokenize(raw);
    if tokens.is_empty() {
        return None;
    }
    let query = tokens
        .iter()
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");
    Some(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(raw: &str) -> Vec<String> {
        search_terms(raw)
    }

    fn matched_text(hit: &SearchHit) -> Vec<String> {
        hit.snippet
            .iter()
            .filter(|s| s.matched)
            .map(|s| s.text.clone())
            .collect()
    }

    fn snippet_text(hit: &SearchHit) -> String {
        hit.snippet.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn search_terms_match_tokenizer_output() {
        assert_eq!(search_terms("RuSt Lang"), vec!["rust", "lang"]);
        // Sub-minimum fragments are dropped, exactly like to_prefix_match.
        assert_eq!(search_terms("a fn"), vec!["fn"]);
        assert!(search_terms("  ").is_empty());
    }

    #[test]
    fn build_hit_reports_first_matching_line_and_highlights_term() {
        let content = "intro line\nthe token lives here\ntrailing";
        let hit = build_hit("b1", "notes.md", content, &terms("token"));
        assert_eq!(hit.buffer_id, "b1");
        assert_eq!(hit.title, "notes.md");
        assert_eq!(hit.line, Some(2));
        assert_eq!(snippet_text(&hit), "the token lives here");
        assert_eq!(matched_text(&hit), vec!["token"]);
    }

    #[test]
    fn build_hit_matches_case_insensitively() {
        let hit = build_hit("b", "t", "A TOKEN here", &terms("token"));
        assert_eq!(hit.line, Some(1));
        assert_eq!(matched_text(&hit), vec!["TOKEN"]);
    }

    #[test]
    fn build_hit_highlights_each_of_multiple_terms() {
        let hit = build_hit("b", "t", "rerank the ceiling", &terms("rerank ceiling"));
        assert_eq!(matched_text(&hit), vec!["rerank", "ceiling"]);
    }

    #[test]
    fn build_hit_trims_leading_whitespace_of_the_line() {
        let hit = build_hit("b", "t", "\t   indented token", &terms("token"));
        assert_eq!(snippet_text(&hit), "indented token");
    }

    #[test]
    fn build_hit_falls_back_to_first_nonempty_line_without_a_match() {
        // Title matched in FTS but no content line contains the term.
        let hit = build_hit("b", "report", "\n\nfirst real line\nmore", &terms("report"));
        assert_eq!(hit.line, None);
        assert_eq!(snippet_text(&hit), "first real line");
        assert!(matched_text(&hit).is_empty());
    }

    #[test]
    fn build_hit_falls_back_to_title_when_content_is_empty() {
        let hit = build_hit("b", "the report title", "", &terms("report"));
        assert_eq!(hit.line, None);
        assert_eq!(snippet_text(&hit), "the report title");
        assert_eq!(matched_text(&hit), vec!["report"]);
    }

    #[test]
    fn build_hit_windows_a_long_line_around_the_match() {
        let prefix = "x".repeat(200);
        let content = format!("{prefix} token tail");
        let hit = build_hit("b", "t", &content, &terms("token"));
        let text = snippet_text(&hit);
        assert!(
            text.starts_with('…'),
            "windowed snippet should lead with an ellipsis"
        );
        assert!(text.contains("token"));
        assert!(text.chars().count() <= SNIPPET_MAX_CHARS + 2);
        assert_eq!(matched_text(&hit), vec!["token"]);
    }

    #[test]
    fn build_hit_overlapping_terms_do_not_double_count() {
        // "tok" and "token" overlap; the merged span highlights "token" once.
        let hit = build_hit("b", "t", "a token here", &terms("tok token"));
        assert_eq!(matched_text(&hit), vec!["token"]);
    }

    #[test]
    fn single_term_becomes_quoted_prefix() {
        assert_eq!(to_prefix_match("tok").as_deref(), Some("\"tok\"*"));
    }

    #[test]
    fn multiple_terms_join_with_space() {
        assert_eq!(
            to_prefix_match("edit buffer").as_deref(),
            Some("\"edit\"* \"buffer\"*"),
        );
    }

    #[test]
    fn lowercases_terms() {
        assert_eq!(to_prefix_match("RuSt").as_deref(), Some("\"rust\"*"));
    }

    #[test]
    fn empty_and_whitespace_yield_none() {
        assert_eq!(to_prefix_match(""), None);
        assert_eq!(to_prefix_match("   \t\n"), None);
    }

    #[test]
    fn single_character_tokens_are_dropped_below_minimum() {
        // "a b c" is all one-character tokens — no usable prefix, no results.
        assert_eq!(to_prefix_match("a b c"), None);
        // A two-character token survives.
        assert_eq!(to_prefix_match("a fn").as_deref(), Some("\"fn\"*"));
    }

    #[test]
    fn fts_operators_are_neutralized_not_executed() {
        // Column filters, NEAR, boolean operators, and quotes must be reduced
        // to plain prefix tokens, never reach MATCH as syntax.
        assert_eq!(
            to_prefix_match("title:secret").as_deref(),
            Some("\"title\"* \"secret\"*"),
        );
        assert_eq!(to_prefix_match("a OR b").as_deref(), Some("\"or\"*"));
        assert_eq!(
            to_prefix_match("\"quoted\"").as_deref(),
            Some("\"quoted\"*"),
        );
        // All fragments here are single characters, so nothing survives.
        assert_eq!(to_prefix_match("a* -b"), None);
    }

    #[test]
    fn punctuation_inside_terms_splits_tokens() {
        assert_eq!(
            to_prefix_match("foo.bar_baz").as_deref(),
            Some("\"foo\"* \"bar\"* \"baz\"*"),
        );
    }

    #[test]
    fn unicode_alphanumerics_are_preserved() {
        assert_eq!(to_prefix_match("café").as_deref(), Some("\"café\"*"));
    }
}
