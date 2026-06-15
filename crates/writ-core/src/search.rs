//! Full-text search query policy.
//!
//! User input from the sidebar search box is raw text. It must never reach
//! the FTS5 `MATCH` parser verbatim: a stray `"`, `*`, `-`, `NEAR`, or column
//! filter (`title:`) either errors the query or silently changes its meaning.
//! This module converts a raw query into a safe prefix-match expression so
//! that typing `tok` finds `token` and `tokenize` (search-as-you-type), backed
//! by the prefix index from migration 030.

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
