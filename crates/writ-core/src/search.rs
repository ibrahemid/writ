//! Full-text search query policy.
//!
//! User input from the sidebar search box is raw text. It must never reach
//! the FTS5 `MATCH` parser verbatim: a stray `"`, `*`, `-`, `NEAR`, or column
//! filter (`title:`) either errors the query or silently changes its meaning.
//! This module converts a raw query into a safe prefix-match expression so
//! that typing `tok` finds `token` and `tokenize` (search-as-you-type), backed
//! by the prefix index from migration 030.
//!
//! Beyond query construction, this module also turns a matched note into a
//! display [`SearchHit`]: the line number of the matching content line and a
//! highlighted snippet built from [`SnippetSegment`]s. The notes index hands
//! over the fragment its own `snippet()` chose, with the matches marked by the
//! index's tokenizer ([`MarkedFragment`]), so a diacritic-only difference
//! (e.g. `cafe` matching `café`) is highlighted as well as found.
//! [`build_hit`], for a caller holding a note's whole text, highlights
//! literally and ASCII-case-insensitively. Snippets are returned as pre-split
//! segments rather than embedded markup so the renderer never has to parse
//! HTML.

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
    /// Canonical path of the note the hit came from, when the index that
    /// produced it is keyed by path (ADR-028 section 7). A result whose note is
    /// not already open is opened by this path; `None` for a hit that carries
    /// only a buffer id.
    #[serde(default)]
    pub path: Option<String>,
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
                path: None,
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
        path: None,
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
    window_segments(&chars, &spans, false, false)
}

/// Cuts `chars` to a window of at most [`SNIPPET_MAX_CHARS`] around the first
/// of `spans`, which are ascending char ranges, and splits it into segments.
///
/// `cut_before` and `cut_after` say the text itself was already cut short at
/// that end, so the window carries an ellipsis there even when it reaches the
/// edge.
fn window_segments(
    chars: &[char],
    spans: &[(usize, usize)],
    cut_before: bool,
    cut_after: bool,
) -> Vec<SnippetSegment> {
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
    if lead || cut_before {
        out.push(ellipsis());
    }
    let mut pos = start;
    for &(s, e) in spans {
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
    if tail || cut_after {
        out.push(ellipsis());
    }
    out
}

/// The mark the notes index asks the full-text `snippet()` function to put
/// before each match. A control character, which note text seldom holds; a
/// note that does cannot have its own told apart from the marks.
pub const FRAGMENT_MATCH_OPEN: char = '\u{2}';
/// The mark after each match.
pub const FRAGMENT_MATCH_CLOSE: char = '\u{3}';
/// The mark `snippet()` puts at an end where it cut the note's text short.
pub const FRAGMENT_CUT: char = '\u{4}';

/// The stretch of a note the full-text index chose to show for a match, with
/// its marks read out ([`FRAGMENT_MATCH_OPEN`], [`FRAGMENT_MATCH_CLOSE`],
/// [`FRAGMENT_CUT`]).
///
/// The index picks the stretch and marks the matches with its own tokenizer, so
/// a diacritic-only difference (`cafe` finding `café`) is highlighted as well
/// as found, and the note's text never has to leave the index to be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedFragment {
    /// The fragment with every mark removed: a run of the note's text exactly
    /// as it stands, which is what lets the index find where it sits. A mark
    /// character the note itself holds inside the fragment is removed with
    /// the rest, and such a fragment is found nowhere in the note.
    pub text: String,
    /// Matched ranges, as character offsets into [`MarkedFragment::text`], in
    /// order.
    pub spans: Vec<(usize, usize)>,
    /// The note's text goes on before the fragment.
    pub cut_before: bool,
    /// The note's text goes on after the fragment.
    pub cut_after: bool,
}

impl MarkedFragment {
    /// Reads the marks out of what `snippet()` returned.
    pub fn read(marked: &str) -> Self {
        let cut_before = marked.starts_with(FRAGMENT_CUT);
        let cut_after = marked.len() > FRAGMENT_CUT.len_utf8() && marked.ends_with(FRAGMENT_CUT);
        let mut text = String::with_capacity(marked.len());
        let mut spans = Vec::new();
        let mut open: Option<usize> = None;
        let mut count = 0usize;
        for ch in marked.chars() {
            match ch {
                FRAGMENT_CUT => {}
                FRAGMENT_MATCH_OPEN => open = Some(count),
                FRAGMENT_MATCH_CLOSE => {
                    if let Some(start) = open.take() {
                        if count > start {
                            spans.push((start, count));
                        }
                    }
                }
                other => {
                    text.push(other);
                    count += 1;
                }
            }
        }
        Self {
            text,
            spans,
            cut_before,
            cut_after,
        }
    }

    /// Character offset of the first match in [`MarkedFragment::text`], or
    /// `None` when nothing in the fragment matched (the query matched the
    /// note's name only).
    pub fn first_match(&self) -> Option<usize> {
        self.spans.first().map(|&(start, _)| start)
    }

    /// The display hit for a note whose text the index returned this fragment
    /// from.
    ///
    /// `line` is the 1-based line of the first match, which the index works
    /// out from the note's text. The snippet is the part of the fragment on
    /// that line, windowed to [`SNIPPET_MAX_CHARS`], with an ellipsis wherever
    /// the line goes on. A fragment with no match falls back as
    /// [`build_hit`] does: its first non-empty line, then the title, with
    /// `terms` highlighted and no line number.
    pub fn into_hit(self, title: &str, line: Option<u32>, terms: &[String]) -> SearchHit {
        let chars: Vec<char> = self.text.chars().collect();
        let Some(first) = self.first_match() else {
            let fallback = self.text.lines().find(|l| !l.trim().is_empty());
            return SearchHit {
                buffer_id: String::new(),
                title: title.to_string(),
                line: None,
                snippet: highlight_window(fallback.unwrap_or(title), terms),
                path: None,
            };
        };

        let line_start = chars[..first]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |at| at + 1);
        let mut line_end = chars[first..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(chars.len(), |at| first + at);
        let reaches_end = line_end == chars.len();
        if line_end > line_start && chars[line_end - 1] == '\r' {
            line_end -= 1;
        }
        let spans: Vec<(usize, usize)> = self
            .spans
            .iter()
            .filter(|&&(s, e)| s < line_end && e > line_start)
            .map(|&(s, e)| (s.max(line_start) - line_start, e.min(line_end) - line_start))
            .collect();
        SearchHit {
            buffer_id: String::new(),
            title: title.to_string(),
            line,
            snippet: window_segments(
                &chars[line_start..line_end],
                &spans,
                line_start == 0 && self.cut_before,
                reaches_end && self.cut_after,
            ),
            path: None,
        }
    }
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

/// How many characters of lead to keep before the first match when windowing a
/// long content line.
const CONTENT_LEAD_CHARS: usize = 32;

/// Builds a highlighted snippet for one workspace content-search match line.
///
/// The literal `query` is highlighted ASCII-case-insensitively (matching the
/// case-insensitive grep matcher; a Unicode-case-only difference is found but
/// not visually highlighted). A line longer than `max_bytes` is windowed around
/// the first match and truncated so the snippet's UTF-8 length never exceeds
/// `max_bytes`, with an ellipsis marking each elided side. The match is always
/// preserved: a long line is elided, never dropped (ADR-026).
pub fn content_snippet(line: &str, query: &str, max_bytes: usize) -> Vec<SnippetSegment> {
    let chars: Vec<char> = line.chars().collect();
    let terms = [query.to_string()];
    let spans = match_spans(&chars, &terms);

    // Choose a window start: lead into the first match if the line is long.
    let mut start = 0usize;
    let mut lead = false;
    if line.len() > max_bytes {
        if let Some(&(first, _)) = spans.first() {
            if first > CONTENT_LEAD_CHARS {
                start = first - CONTENT_LEAD_CHARS;
                lead = true;
            }
        }
    }
    if !lead {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }

    // Extend the window char by char until the next character would push the
    // UTF-8 byte length past the cap.
    let ellipsis_bytes = "…".len();
    let budget = max_bytes.saturating_sub(if lead { ellipsis_bytes } else { 0 });
    let mut used = 0usize;
    let mut end = start;
    while end < chars.len() {
        let cb = chars[end].len_utf8();
        if used + cb > budget {
            break;
        }
        used += cb;
        end += 1;
    }
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

    fn fragment(marked: &str) -> MarkedFragment {
        MarkedFragment::read(
            &marked
                .replace('[', "\u{2}")
                .replace(']', "\u{3}")
                .replace('~', "\u{4}"),
        )
    }

    #[test]
    fn a_fragment_reads_its_marks_out_of_the_text() {
        let read = fragment("a [rust] and [rusty] b");
        assert_eq!(read.text, "a rust and rusty b");
        assert_eq!(read.spans, vec![(2, 6), (11, 16)]);
        assert_eq!(read.first_match(), Some(2));
        assert!(!read.cut_before && !read.cut_after);
    }

    #[test]
    fn a_fragment_knows_where_the_note_was_cut() {
        let read = fragment("~x [y] z~");
        assert_eq!(read.text, "x y z");
        assert!(read.cut_before);
        assert!(read.cut_after);
    }

    #[test]
    fn a_fragment_hit_shows_the_line_its_first_match_is_on() {
        let hit = fragment("first line\nthe [rerank] ceiling here\ntail").into_hit(
            "notes.md",
            Some(2),
            &terms("rerank"),
        );
        assert_eq!(hit.title, "notes.md");
        assert_eq!(hit.line, Some(2));
        assert_eq!(snippet_text(&hit), "the rerank ceiling here");
        assert_eq!(matched_text(&hit), vec!["rerank"]);
    }

    #[test]
    fn a_fragment_hit_highlights_the_whole_token_the_index_matched() {
        let hit = fragment("the [café] opens early").into_hit("a.md", Some(1), &terms("cafe"));
        assert_eq!(matched_text(&hit), vec!["café"]);
    }

    #[test]
    fn a_fragment_hit_drops_a_windows_line_ending() {
        let hit = fragment("a\r\n[b]c\r\nd").into_hit("a.md", Some(2), &terms("b"));
        assert_eq!(snippet_text(&hit), "bc");
    }

    #[test]
    fn a_fragment_cut_mid_line_says_so() {
        let hit =
            fragment("~middle of [word] here\nnext line").into_hit("a.md", Some(4), &terms("word"));
        assert_eq!(hit.snippet.first().map(|s| s.text.as_str()), Some("…"));
        assert_eq!(snippet_text(&hit), "…middle of word here");
        let hit = fragment("start [word] goes on~").into_hit("a.md", Some(1), &terms("word"));
        assert_eq!(snippet_text(&hit), "start word goes on…");
    }

    #[test]
    fn a_fragment_hit_windows_a_long_line_around_the_match() {
        let long = format!("{} [needle] {}", "x".repeat(200), "y".repeat(200));
        let hit = fragment(&long).into_hit("a.md", Some(1), &terms("needle"));
        assert_eq!(hit.snippet.first().map(|s| s.text.as_str()), Some("…"));
        assert_eq!(hit.snippet.last().map(|s| s.text.as_str()), Some("…"));
        assert_eq!(matched_text(&hit), vec!["needle"]);
        let shown: usize = hit
            .snippet
            .iter()
            .filter(|s| s.text != "…")
            .map(|s| s.text.chars().count())
            .sum();
        assert!(shown <= SNIPPET_MAX_CHARS);
    }

    #[test]
    fn a_fragment_with_no_match_falls_back_like_build_hit() {
        let hit = fragment("\n\nfirst words\nmore").into_hit("rust.md", None, &terms("rust"));
        assert_eq!(hit.line, None);
        assert_eq!(snippet_text(&hit), "first words");

        let hit = fragment("").into_hit("rust notes.md", None, &terms("rust"));
        assert_eq!(snippet_text(&hit), "rust notes.md");
        assert_eq!(matched_text(&hit), vec!["rust"]);
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

    fn snippet_str(segs: &[SnippetSegment]) -> String {
        segs.iter().map(|s| s.text.as_str()).collect()
    }

    fn snippet_matched(segs: &[SnippetSegment]) -> Vec<String> {
        segs.iter()
            .filter(|s| s.matched)
            .map(|s| s.text.clone())
            .collect()
    }

    #[test]
    fn content_snippet_highlights_literal_query() {
        let segs = content_snippet("let token = 1;", "token", 512);
        assert_eq!(snippet_str(&segs), "let token = 1;");
        assert_eq!(snippet_matched(&segs), vec!["token"]);
    }

    #[test]
    fn content_snippet_is_case_insensitive() {
        let segs = content_snippet("The TOKEN here", "token", 512);
        assert_eq!(snippet_matched(&segs), vec!["TOKEN"]);
    }

    #[test]
    fn content_snippet_elides_long_line_but_keeps_match() {
        let prefix = "x".repeat(2000);
        let line = format!("{prefix} NEEDLE tail");
        let segs = content_snippet(&line, "NEEDLE", 512);
        let text = snippet_str(&segs);
        assert!(text.starts_with('…'), "long line should lead with ellipsis");
        assert!(text.contains("NEEDLE"), "the match must never be dropped");
        assert!(
            text.len() <= 512 + "…".len() * 2,
            "snippet byte length {} exceeds cap",
            text.len()
        );
        assert_eq!(snippet_matched(&segs), vec!["NEEDLE"]);
    }

    #[test]
    fn content_snippet_truncates_long_line_without_match_window() {
        // Match near the front: no leading ellipsis, but a trailing one.
        let line = format!("NEEDLE {}", "y".repeat(2000));
        let segs = content_snippet(&line, "NEEDLE", 512);
        let text = snippet_str(&segs);
        assert!(!text.starts_with('…'));
        assert!(text.ends_with('…'));
        assert!(text.contains("NEEDLE"));
    }

    #[test]
    fn content_snippet_multibyte_never_splits_a_char() {
        // A line of multibyte chars with a match; truncation must land on a char
        // boundary (no panic, valid UTF-8).
        let line = format!("{} café done", "é".repeat(400));
        let segs = content_snippet(&line, "café", 64);
        let text = snippet_str(&segs);
        assert!(text.is_char_boundary(text.len()));
        assert!(text.len() <= 64 + "…".len() * 2);
    }
}
