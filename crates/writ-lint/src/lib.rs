//! Local spell-check and mechanical-writing engine for Writ.
//!
//! Spelling runs on [spellbook](https://github.com/helix-editor/spellbook)
//! against bundled SCOWL Hunspell dictionaries, one per [`Dialect`], with the
//! words Harper's dictionary accepted added to them. Four
//! mechanical rules run beside it: repeated words, repeated spaces, a wrong
//! apostrophe, and a/an. Style and readability rules are not part of the
//! engine: Writ flags mistakes, not prose taste. `docs/adr/050-the-spell-check-engine.md`
//! records the design.
//!
//! # What is read
//!
//! The text is parsed as Markdown and only prose is checked: code, HTML, math,
//! front matter, URLs, email addresses, paths and the target of an unpiped
//! wikilink are skipped. Spelling judges only words written in ASCII letters,
//! so Arabic, Cyrillic or CJK text is never flagged as misspelled.
//!
//! # Coordinate system
//!
//! The engine works in byte offsets. CodeMirror 6, the editor that consumes
//! these results, measures positions in **UTF-16 code units**. [`check`]
//! converts every range to UTF-16 offsets before returning, so the frontend
//! never has to know that the two systems differ.
//!
//! # Crate boundary
//!
//! `writ-lint` depends on `writ-core`, `spellbook` and `pulldown-cmark`. It
//! never imports Tauri; the IPC adapter lives in `writ-tauri`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod an_a;
mod markdown;
mod rules;
mod spelling;
mod tokens;

use serde::{Deserialize, Serialize};

/// The English variant the spell checker validates against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Dialect {
    /// United States English (SCOWL `en_US`).
    #[default]
    American,
    /// British English with -ise spellings (SCOWL `en_GB-ise`).
    British,
    /// Canadian English (SCOWL `en_CA`).
    Canadian,
    /// Australian English (SCOWL `en_AU`).
    Australian,
}

/// Configuration for a single [`check`] call.
#[derive(Debug, Clone, Default)]
pub struct LintConfig {
    /// English variant the spell checker validates against.
    pub dialect: Dialect,
    /// Words the user has chosen never to flag. Matched case-sensitively
    /// against the stored form first, then case-insensitively.
    pub ignored_words: Vec<String>,
}

/// Maps an accepted identifier to a [`Dialect`].
///
/// Accepts `american`, `british`, `canadian`, `australian` (case-insensitive).
/// Anything else falls back to [`Dialect::American`].
pub fn dialect_from_str(s: &str) -> Dialect {
    match s.trim().to_ascii_lowercase().as_str() {
        "british" => Dialect::British,
        "canadian" => Dialect::Canadian,
        "australian" => Dialect::Australian,
        _ => Dialect::American,
    }
}

/// A single lint, in CodeMirror 6-native UTF-16 coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintResult {
    /// Start of the flagged range, in UTF-16 code units from document start.
    pub from_utf16: usize,
    /// End of the flagged range (exclusive), in UTF-16 code units.
    pub to_utf16: usize,
    /// Category key: `Spelling`, `Repetition`, `Formatting`, `Typo` or
    /// `Miscellaneous` (a/an).
    pub kind: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// Replacement candidates for the flagged range, most-preferred first.
    /// Each entry is the full text the range should become.
    pub suggestions: Vec<String>,
    /// Whether Writ trusts this lint enough to pre-select it in "Fix all":
    /// always for the mechanical rules; for spelling, only when the dictionary
    /// offers exactly one suggestion and it is one edit away.
    pub confident: bool,
}

/// A lint in byte offsets, before conversion to UTF-16.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub start: usize,
    pub end: usize,
    pub kind: &'static str,
    pub message: String,
    pub suggestions: Vec<String>,
    pub confident: bool,
}

/// Runs the spell checker and mechanical rules over `text`.
///
/// Returns the flagged ranges in UTF-16 coordinates, sorted by start offset.
/// Every lint carries at least one suggestion: a misspelling the dictionary
/// has no suggestion for is not reported. Spelling lints on ALLCAPS or
/// mid-word-capitalized tokens (`API`, `SolidJS`, `useSignal`) and on ignored
/// words are skipped.
pub fn check(text: &str, config: &LintConfig) -> Vec<LintResult> {
    if text.is_empty() {
        return Vec::new();
    }

    let runs = markdown::prose_runs(text);
    let mut findings: Vec<Finding> = Vec::new();
    let mut words: Vec<tokens::Token> = Vec::new();
    for run in &runs {
        let run_tokens = tokens::tokenize(text, run);
        rules::repeated_words(text, &run_tokens, &mut findings);
        rules::spaces(text, run, &mut findings);
        rules::apostrophes(text, run, &mut findings);
        an_a::lint(text, &run_tokens, config.dialect, &mut findings);
        words.extend(run_tokens);
    }

    let typos: Vec<(usize, usize)> = findings
        .iter()
        .filter(|f| f.kind == rules::TYPO)
        .map(|f| (f.start, f.end))
        .collect();
    spelling::lint(text, &words, config, &typos, &mut findings);

    let mut out = to_utf16(text, findings);
    out.sort_by(|a, b| {
        a.from_utf16
            .cmp(&b.from_utf16)
            .then(a.to_utf16.cmp(&b.to_utf16))
    });
    out
}

/// Converts byte ranges to UTF-16 ranges in one pass over the text.
fn to_utf16(text: &str, findings: Vec<Finding>) -> Vec<LintResult> {
    let mut offsets: Vec<usize> = findings.iter().flat_map(|f| [f.start, f.end]).collect();
    offsets.sort_unstable();
    offsets.dedup();

    let mut table: Vec<(usize, usize)> = Vec::with_capacity(offsets.len());
    let mut chars = text.char_indices();
    let mut byte = 0usize;
    let mut utf16 = 0usize;
    for &target in &offsets {
        while byte < target {
            let Some((at, c)) = chars.next() else {
                break;
            };
            utf16 += c.len_utf16();
            byte = at + c.len_utf8();
        }
        table.push((target, utf16));
    }
    let lookup = |byte: usize| -> usize {
        table
            .binary_search_by_key(&byte, |&(b, _)| b)
            .map_or(0, |i| table[i].1)
    };

    findings
        .into_iter()
        .map(|f| LintResult {
            from_utf16: lookup(f.start),
            to_utf16: lookup(f.end),
            kind: f.kind.to_owned(),
            message: f.message,
            suggestions: f.suggestions,
            confident: f.confident,
        })
        .collect()
}

#[cfg(test)]
mod tests;
