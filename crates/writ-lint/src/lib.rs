//! Local spell-check and mechanical-writing engine for Writ.
//!
//! Wraps [Harper](https://writewithharper.com) to lint the user's document for
//! spelling and a small set of mechanical rules (repeated words, repeated
//! spaces, wrong apostrophe, a/an). Style and readability rules are off: Writ
//! flags mistakes, not prose taste.
//!
//! # Coordinate system
//!
//! Harper reports [`harper_core::Span`] positions as **character** indices into
//! the source. CodeMirror 6 — the editor that consumes these results — measures
//! positions in **UTF-16 code units**. [`check`] converts every span to UTF-16
//! offsets before returning, so the frontend never has to know that the two
//! systems differ. The conversion accumulates [`char::len_utf16`] over the
//! source characters, which is correct for astral-plane emoji, Arabic, and CJK
//! alike.
//!
//! # Crate boundary
//!
//! `writ-lint` depends on `writ-core` and `harper-core` only. It never imports
//! Tauri; the IPC adapter lives in `writ-tauri`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use harper_core::linting::{LintGroup, Linter, Suggestion};
use harper_core::spell::FstDictionary;
use harper_core::Document;
use serde::{Deserialize, Serialize};

pub use harper_core::Dialect;

/// Configuration for a single [`check`] call.
#[derive(Debug, Clone)]
pub struct LintConfig {
    /// English dialect the spell checker validates against.
    pub dialect: Dialect,
    /// Words the user has chosen never to flag. Matched case-sensitively
    /// against the stored form first, then case-insensitively.
    pub ignored_words: Vec<String>,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            dialect: Dialect::American,
            ignored_words: Vec::new(),
        }
    }
}

/// Maps an accepted dialect identifier to Harper's [`Dialect`].
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
    /// Harper category key, e.g. `Spelling`, `Repetition`, `Formatting`.
    pub kind: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// Replacement candidates for the flagged range, most-preferred first.
    /// Each entry is the full text the range should become.
    pub suggestions: Vec<String>,
    /// Whether Writ trusts this lint enough to auto-apply it in "Fix all".
    pub confident: bool,
}

/// Runs the spell checker and mechanical rules over `text`.
///
/// Returns the flagged ranges in UTF-16 coordinates, sorted by start offset.
/// Only spelling lints that carry at least one suggestion, plus the enabled
/// mechanical rules, are reported; everything without a suggestion is dropped.
/// Spelling lints on ALLCAPS or mid-word-capitalized tokens (`API`, `SolidJS`,
/// `useSignal`) and on ignored words are skipped.
pub fn check(text: &str, config: &LintConfig) -> Vec<LintResult> {
    if text.is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = text.chars().collect();
    // utf16_prefix[i] is the UTF-16 offset of character i; the trailing entry
    // is the document's total UTF-16 length. A char-index span [start, end)
    // maps to [utf16_prefix[start], utf16_prefix[end]).
    let mut utf16_prefix: Vec<usize> = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0usize;
    utf16_prefix.push(0);
    for c in &chars {
        acc += c.len_utf16();
        utf16_prefix.push(acc);
    }

    let dictionary = FstDictionary::curated();
    let mut group = LintGroup::new_curated(dictionary, config.dialect);
    // Start from nothing, then turn on only spelling and the mechanical rules.
    group.set_all_rules_to(Some(false));
    group.config.set_rule_enabled("SpellCheck", true);
    group.config.set_rule_enabled("RepeatedWords", true);
    group.config.set_rule_enabled("Spaces", true);
    group.config.set_rule_enabled("WrongApostrophe", true);
    group.config.set_rule_enabled("AnA", true);

    let document = Document::new_markdown_default_curated(text);
    let lints = group.lint(&document);

    let mut out: Vec<LintResult> = Vec::with_capacity(lints.len());
    for lint in lints {
        let start = lint.span.start;
        let end = lint.span.end;
        if start > end || end > chars.len() {
            continue;
        }

        let original: String = chars[start..end].iter().collect();
        let suggestions: Vec<String> = lint
            .suggestions
            .iter()
            .map(|s| suggestion_to_string(s, &original))
            .collect();
        if suggestions.is_empty() {
            continue;
        }

        let is_spelling = lint.lint_kind.is_spelling();
        if is_spelling {
            if has_internal_uppercase(&original) {
                continue;
            }
            if is_ignored(&original, &config.ignored_words) {
                continue;
            }
        }

        // Every lint that reaches here carries a suggestion (empties were
        // dropped above), so spelling-with-a-fix and mechanical rules are all
        // confident.
        let confident = true;

        out.push(LintResult {
            from_utf16: utf16_prefix[start],
            to_utf16: utf16_prefix[end],
            kind: lint.lint_kind.to_string_key(),
            message: lint.message,
            suggestions,
            confident,
        });
    }

    out.sort_by(|a, b| {
        a.from_utf16
            .cmp(&b.from_utf16)
            .then(a.to_utf16.cmp(&b.to_utf16))
    });
    out
}

/// Converts a Harper suggestion into the full text the flagged range should
/// become. `Remove` yields an empty string; `InsertAfter` appends to the
/// original span so the range replacement carries the same effect.
fn suggestion_to_string(suggestion: &Suggestion, original: &str) -> String {
    match suggestion {
        Suggestion::ReplaceWith(chars) => chars.iter().collect(),
        Suggestion::Remove => String::new(),
        Suggestion::InsertAfter(chars) => {
            let mut replacement = original.to_string();
            replacement.extend(chars.iter());
            replacement
        }
    }
}

/// True when a token carries an uppercase letter anywhere but the first
/// position — the shape of ALLCAPS acronyms (`API`) and camel/Pascal-case
/// identifiers (`SolidJS`, `useSignal`) that spell check must not flag.
fn has_internal_uppercase(word: &str) -> bool {
    word.chars().skip(1).any(|c| c.is_uppercase())
}

/// True when `word` is on the ignore list, matching the stored form
/// case-sensitively first, then case-insensitively.
fn is_ignored(word: &str, ignored: &[String]) -> bool {
    if ignored.iter().any(|w| w == word) {
        return true;
    }
    let lower = word.to_lowercase();
    ignored.iter().any(|w| w.to_lowercase() == lower)
}

#[cfg(test)]
mod tests;
