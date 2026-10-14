//! Mechanical rules: repeated words, repeated spaces and wrong apostrophes.

use std::ops::Range;

use crate::tokens::{gap_is_whitespace, is_latin_word, Token, TokenKind};
use crate::Finding;

/// Kind key of a typo-shaped lint (a wrong apostrophe).
pub(crate) const TYPO: &str = "Typo";

/// Words English legitimately writes twice in a row ("she had had enough",
/// "he said that that was fine", "bye bye").
const LEGITIMATE_DOUBLES: &[&str] = &["had", "that", "bye", "blah", "knock", "tsk", "yada"];

/// Endings of contractions and possessives a wrong apostrophe can precede.
const CONTRACTION_ENDINGS: &[&str] = &["d", "ll", "m", "re", "s", "t", "ve"];

/// Characters typed in place of an apostrophe: semicolon, acute accent, backtick.
const APOSTROPHE_STAND_INS: &[char] = &[';', '\u{00B4}', '`'];

/// True for `ha`, `hah`, `haha` and longer alternations of `h` and `a`.
pub(crate) fn is_informal_laughter(word: &str) -> bool {
    word.chars().count() >= 2
        && word.chars().enumerate().all(|(i, c)| {
            if i % 2 == 0 {
                matches!(c, 'h' | 'H')
            } else {
                matches!(c, 'a' | 'A')
            }
        })
}

fn char_before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn char_after(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// Flags a word written twice with only whitespace between, as one range
/// whose replacement is the word once.
pub(crate) fn repeated_words(text: &str, tokens: &[Token], out: &mut Vec<Finding>) {
    let mut i = 0usize;
    while i + 1 < tokens.len() {
        let (a, b) = (tokens[i], tokens[i + 1]);
        let first = a.text(text);
        let second = b.text(text);
        let is_repeat = a.kind == TokenKind::Word
            && b.kind == TokenKind::Word
            && is_latin_word(first)
            && first.eq_ignore_ascii_case(second)
            && gap_is_whitespace(text, a.end, b.start)
            && !is_informal_laughter(first)
            && char_before(text, a.start) != Some('-')
            && char_after(text, b.end) != Some('-')
            && !LEGITIMATE_DOUBLES.contains(&first.to_ascii_lowercase().as_str());
        if is_repeat {
            out.push(Finding {
                start: a.start,
                end: b.end,
                kind: "Repetition",
                message: "Did you mean to repeat this word?".to_owned(),
                suggestions: vec![first.to_owned()],
                confident: true,
            });
            i += 2;
        } else {
            i += 1;
        }
    }
}

fn ends_sentence(c: char) -> bool {
    matches!(c, '.' | '!' | '?')
}

fn closes_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '\u{201D}' | '\u{2019}' | ')' | ']')
}

/// True when the spaces starting at `at` follow the end of a sentence, where
/// two spaces are a typing convention rather than a mistake.
fn follows_sentence_end(text: &str, at: usize) -> bool {
    let mut before = text[..at].chars().rev();
    match before.next() {
        Some(c) if ends_sentence(c) => true,
        Some(c) if closes_quote(c) => before.next().is_some_and(ends_sentence),
        _ => false,
    }
}

/// Flags runs of spaces between two words, and a space between a word and
/// the punctuation that ends its sentence.
pub(crate) fn spaces(text: &str, run: &Range<usize>, out: &mut Vec<Finding>) {
    let bytes = text.as_bytes();
    let mut i = run.start;
    while i < run.end {
        if bytes[i] != b' ' {
            i += 1;
            continue;
        }
        let start = i;
        while i < run.end && bytes[i] == b' ' {
            i += 1;
        }
        let end = i;
        let (Some(prev), Some(next)) = (char_before(text, start), char_after(text, end)) else {
            continue;
        };
        if prev.is_whitespace() || next.is_whitespace() {
            continue;
        }
        let closes_sentence = end < run.end
            && ends_sentence(next)
            && prev.is_alphabetic()
            && char_after(text, end + next.len_utf8())
                .is_none_or(|c| c.is_whitespace() || closes_quote(c));
        if closes_sentence {
            out.push(Finding {
                start,
                end,
                kind: "Formatting",
                message: "Unnecessary space at the end of the sentence.".to_owned(),
                suggestions: vec![String::new()],
                confident: true,
            });
        } else if end - start > 1 && !follows_sentence_end(text, start) {
            let count = end - start;
            out.push(Finding {
                start,
                end,
                kind: "Formatting",
                message: format!("There are {count} spaces where there should be only one."),
                suggestions: vec![" ".to_owned()],
                confident: true,
            });
        }
    }
}

/// Flags a semicolon, acute accent or backtick typed where a contraction or
/// possessive needs an apostrophe: `don;t`, `it´s`, `we`ll`.
pub(crate) fn apostrophes(text: &str, run: &Range<usize>, out: &mut Vec<Finding>) {
    let slice = &text[run.start..run.end];
    for (offset, c) in slice.char_indices() {
        if !APOSTROPHE_STAND_INS.contains(&c) {
            continue;
        }
        let mark = run.start + offset;
        let after = mark + c.len_utf8();

        let base_start = text[run.start..mark]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| ch.is_ascii_alphabetic())
            .last()
            .map(|(i, _)| run.start + i);
        let Some(base_start) = base_start else {
            continue;
        };
        if char_before(text, base_start).is_some_and(char::is_alphanumeric) {
            continue;
        }

        let ending_len = text[after..run.end]
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .count();
        let ending_end = after + ending_len;
        if ending_len == 0 || char_after(text, ending_end).is_some_and(char::is_alphanumeric) {
            continue;
        }
        let ending = &text[after..ending_end];
        if !CONTRACTION_ENDINGS.contains(&ending.to_ascii_lowercase().as_str()) {
            continue;
        }

        let replacement = format!("{}'{}", &text[base_start..mark], ending);
        out.push(Finding {
            start: base_start,
            end: ending_end,
            kind: TYPO,
            message: format!("Did you mean `{replacement}`?"),
            suggestions: vec![replacement],
            confident: true,
        });
    }
}
