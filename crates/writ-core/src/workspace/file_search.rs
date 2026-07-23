//! Fuzzy file-name ranking for workspace name search.
//!
//! Pure policy: given a query and a set of workspace-relative paths, decide
//! which paths the query matches as a subsequence and rank them. No I/O, no
//! walking — the caller supplies the candidate paths from the in-memory index.
//!
//! Scoring is a greedy, deterministic subsequence match. A query matches a path
//! when its characters appear in order (ASCII-case-insensitively) somewhere in
//! the path. A match inside the file name is scored on the file name alone and
//! earns a large bonus, so `app/cfg.rs` beats `cfg/deep/other.rs` for `cfg`;
//! only when the file name does not contain the query is the whole path scored.
//! Within whichever text is scored, matches score higher when they are
//! consecutive or land on a word boundary (after `/`, `_`, `-`, `.`, or space)
//! and when they start the text. Ties break toward the shorter path, then
//! lexicographically, so ranking is stable across runs and platforms.

use serde::{Deserialize, Serialize};

/// A ranked workspace file-name match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHit {
    /// Workspace-relative path of the file (forward-slash separated).
    pub path: String,
    /// File name (the final path segment).
    pub name: String,
    /// Match score; higher ranks first.
    pub score: i32,
}

/// Bonus for a match character immediately following the previous match.
const CONSECUTIVE_BONUS: i32 = 20;
/// Bonus for a match at a word boundary (after a separator).
const BOUNDARY_BONUS: i32 = 10;
/// Bonus for a match at the very first character of the scored text.
const FIRST_CHAR_BONUS: i32 = 10;
/// Bonus added when the query matches within the file name.
const BASENAME_BONUS: i32 = 40;
/// Per-character penalty for a gap between matched characters.
const GAP_PENALTY: i32 = 1;
/// Penalty cap so a very long directory prefix cannot dominate the score.
const MAX_GAP_PENALTY: i32 = 20;

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | '.' | ' ')
}

/// Greedy subsequence score of `query` (already lowercased chars) over `text`.
/// Returns `None` when `query` is not a subsequence of `text`. Consecutive
/// matches and matches on a word boundary score higher; gaps are penalized.
fn greedy_score(query: &[char], text: &str) -> Option<i32> {
    if query.is_empty() {
        return None;
    }

    let chars: Vec<char> = text.chars().collect();
    let mut score = 0i32;
    let mut qi = 0usize;
    let mut prev_match: Option<usize> = None;

    for (ci, &raw) in chars.iter().enumerate() {
        if qi >= query.len() {
            break;
        }
        if raw.to_ascii_lowercase() != query[qi] {
            continue;
        }

        score += 1;

        if ci == 0 {
            score += FIRST_CHAR_BONUS + BOUNDARY_BONUS;
        } else if is_separator(chars[ci - 1]) {
            score += BOUNDARY_BONUS;
        }

        if let Some(p) = prev_match {
            if ci == p + 1 {
                score += CONSECUTIVE_BONUS;
            } else {
                let gap = (ci - p - 1) as i32;
                score -= (gap * GAP_PENALTY).min(MAX_GAP_PENALTY);
            }
        }

        prev_match = Some(ci);
        qi += 1;
    }

    if qi == query.len() {
        Some(score)
    } else {
        None
    }
}

/// Returns the byte-agnostic file-name segment of a forward/back-slash path.
fn basename(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Scores `path` against `query` as a subsequence, or `None` when the query is
/// empty/blank or does not match. A path whose file-name segment also contains
/// the query as a subsequence earns an additional [`BASENAME_BONUS`].
pub fn subsequence_score(query: &str, path: &str) -> Option<i32> {
    let q: Vec<char> = query
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if q.is_empty() {
        return None;
    }

    // Prefer a file-name match: score it on the file name alone with a large
    // bonus so a name hit always outranks a directory-only hit. Fall back to the
    // whole path only when the file name does not contain the query.
    if let Some(name_score) = greedy_score(&q, basename(path)) {
        return Some(name_score + BASENAME_BONUS);
    }
    greedy_score(&q, path)
}

/// Ranks `candidates` (`(path, name)` pairs) against `query`, returning the top
/// `limit` matches sorted by score descending, then shorter path, then path
/// lexicographically. An empty or blank query returns no matches. Candidates are
/// borrowed and only matches allocate, so scanning a large index does not clone
/// every non-matching path.
pub fn rank_file_hits<'a, I>(query: &str, candidates: I, limit: usize) -> Vec<FileHit>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut hits: Vec<FileHit> = candidates
        .into_iter()
        .filter_map(|(path, name)| {
            subsequence_score(query, path).map(|score| FileHit {
                path: path.to_string(),
                name: name.to_string(),
                score,
            })
        })
        .collect();

    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
    });
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(query: &str, candidates: &[(&str, &str)], limit: usize) -> Vec<FileHit> {
        rank_file_hits(query, candidates.iter().copied(), limit)
    }

    #[test]
    fn empty_query_never_matches() {
        assert_eq!(subsequence_score("", "src/main.rs"), None);
        assert_eq!(subsequence_score("   ", "src/main.rs"), None);
    }

    #[test]
    fn non_subsequence_returns_none() {
        assert_eq!(subsequence_score("xyz", "src/main.rs"), None);
        // Out of order: `nm` is not a subsequence of "main".
        assert_eq!(subsequence_score("nm", "main"), None);
    }

    #[test]
    fn subsequence_matches_in_order() {
        assert!(subsequence_score("mn", "main.rs").is_some());
        assert!(subsequence_score("srcmain", "src/main.rs").is_some());
    }

    #[test]
    fn scoring_is_deterministic() {
        let a = subsequence_score("main", "src/main.rs");
        let b = subsequence_score("main", "src/main.rs");
        assert_eq!(a, b);
        assert!(a.is_some());
    }

    #[test]
    fn consecutive_beats_scattered() {
        let consecutive = subsequence_score("main", "main.rs").unwrap();
        let scattered = subsequence_score("main", "m_a_i_n.rs").unwrap();
        assert!(
            consecutive > scattered,
            "consecutive {consecutive} should beat scattered {scattered}"
        );
    }

    #[test]
    fn basename_match_outranks_directory_only_match() {
        // Both contain "cfg" as a subsequence, but only the first has it in the
        // file name.
        let in_name = subsequence_score("cfg", "app/cfg.rs").unwrap();
        let in_dir = subsequence_score("cfg", "cfg/deep/nested/other.rs").unwrap();
        assert!(
            in_name > in_dir,
            "name match {in_name} should beat dir match {in_dir}"
        );
    }

    #[test]
    fn word_boundary_scores_higher() {
        // Query "m" hitting the start of the "main" segment (after '/') should
        // beat hitting a mid-word 'm'.
        let boundary = subsequence_score("m", "a/main").unwrap();
        let midword = subsequence_score("m", "gamma").unwrap();
        assert!(boundary > midword);
    }

    #[test]
    fn ranking_orders_by_score_then_path() {
        let candidates = [
            ("src/main.rs", "main.rs"),
            ("main.rs", "main.rs"),
            ("lib/mango/notes.md", "notes.md"),
        ];
        let hits = rank("main", &candidates, 10);
        // "main.rs" (shorter, basename match) ranks above "src/main.rs".
        assert_eq!(hits[0].path, "main.rs");
        assert_eq!(hits[1].path, "src/main.rs");
        // "notes.md" does not contain "main" as a subsequence.
        assert!(hits.iter().all(|h| h.path != "lib/mango/notes.md"));
    }

    #[test]
    fn ranking_tie_breaks_shorter_then_lexicographic() {
        // Identical basename and query; scores tie, so length then lexical order
        // decides.
        let candidates = [
            ("zzz/app.rs", "app.rs"),
            ("aaa/app.rs", "app.rs"),
            ("app.rs", "app.rs"),
        ];
        let hits = rank("app", &candidates, 10);
        assert_eq!(hits[0].path, "app.rs");
        assert_eq!(hits[1].path, "aaa/app.rs");
        assert_eq!(hits[2].path, "zzz/app.rs");
    }

    #[test]
    fn ranking_respects_limit() {
        let paths: Vec<String> = (0..50).map(|i| format!("dir{i}/main.rs")).collect();
        let candidates: Vec<(&str, &str)> = paths.iter().map(|p| (p.as_str(), "main.rs")).collect();
        let hits = rank_file_hits("main", candidates, 5);
        assert_eq!(hits.len(), 5);
    }

    #[test]
    fn ranking_empty_query_returns_nothing() {
        assert!(rank("", &[("main.rs", "main.rs")], 10).is_empty());
    }
}
