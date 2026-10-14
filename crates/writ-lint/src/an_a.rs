// Ported from Harper (https://github.com/Automattic/harper), harper-core 2.11.0,
// commit c821f511f83c24401993e207f7e5cdb51ae872e6: src/indefinite_article.rs and
// src/linting/an_a.rs, with their tests.
//
// Copyright 2024 Automattic Inc.
// Licensed under the Apache License, Version 2.0; the licence text is in
// THIRD-PARTY-NOTICES.md under "Bundled files".
//
// Modified for Writ: the rule reads writ-lint's tokens instead of a Harper
// Document; an article and its noun pair only when whitespace alone separates
// them; and a noun whose initial sound is ambiguous skips that one pair, where
// the original stopped the rule for the rest of the document.

//! The a/an rule: flags `a` before a vowel sound and `an` before a consonant
//! sound.

use crate::tokens::{gap_is_whitespace, Token, TokenKind};
use crate::{Dialect, Finding};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum InitialSound {
    Vowel,
    Consonant,
    Either,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Case {
    Upper,
    Lower,
}

fn case_of(c: char) -> Option<Case> {
    if c.is_uppercase() {
        Some(Case::Upper)
    } else if c.is_lowercase() {
        Some(Case::Lower)
    } else {
        None
    }
}

fn is_vowel(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Whether `word` begins with a vowel sound. `None` for an empty word.
///
/// Harper's notes: produced through trial and error; matches 99.71% of vowels
/// and 99.77% of non-vowels in the Carnegie Mellon University pronunciation
/// dataset.
fn starts_with_vowel(word: &[char], dialect: Dialect) -> Option<InitialSound> {
    if word.is_empty() {
        return None;
    }

    if matches!(word, ['L', 'E', 'D'] | ['S', 'Q', 'L'] | ['U', 'R', 'L']) {
        return Some(InitialSound::Either);
    }

    // The leading chunk of a partial initialism: `RFL` from `RFLink`, `m` from
    // `mDNS`.
    let word = {
        let casing: Vec<Option<Case>> = word.iter().map(|&c| case_of(c)).collect();
        match casing.as_slice() {
            [Some(first), Some(Case::Upper), ..] => {
                let first = *first;
                &word[0..casing
                    .iter()
                    .position(|c| *c != Some(first))
                    .unwrap_or(word.len())]
            }
            _ => word,
        }
    };

    let is_likely_initialism = word.iter().all(|c| !c.is_alphabetic() || c.is_uppercase());

    if word.len() == 1 || (is_likely_initialism && !is_likely_acronym(word)) {
        return Some(
            if matches!(
                word[0].to_ascii_uppercase(),
                'A' | 'E' | 'F' | 'H' | 'I' | 'L' | 'M' | 'N' | 'O' | 'R' | 'S' | 'X'
            ) {
                InitialSound::Vowel
            } else {
                InitialSound::Consonant
            },
        );
    }

    let lower: Vec<char> = word.iter().flat_map(|c| c.to_lowercase()).collect();
    let word = lower.as_slice();

    if matches!(word, ['u', 'b', 'i', ..]) {
        return Some(InitialSound::Either);
    }

    // `mp3` reads as `MP3`: the leading `m` is the letter name "em".
    if matches!(word, ['m', 'p', digit, ..] if digit.is_ascii_digit()) {
        return Some(InitialSound::Vowel);
    }

    // `npm` is an all-lowercase initialism read letter by letter.
    if matches!(word, ['n', 'p', 'm']) {
        return Some(InitialSound::Vowel);
    }

    if matches!(word, ['e', 'u', 'l', 'e', ..]) {
        return Some(InitialSound::Vowel);
    }

    if matches!(
        word,
        ['u', 'k', ..]
            | ['u', 'd', 'e', ..]
            | ['e', 'u', 'p', 'h', ..]
            | ['e', 'u', 'g' | 'l' | 'c', ..]
            | ['o', 'n', 'e', ..]
            | ['o', 'n', 'c', 'e']
    ) {
        return Some(InitialSound::Consonant);
    }

    if matches!(
        word,
        ['h', 'o', 'u', 'r', ..]
            | ['u', 'n', 'i', 'n' | 'm', ..]
            | ['u', 'n', 'a' | 'u', ..]
            | ['u', 'r', 'b', ..]
            | ['i', 'n', 't', ..]
    ) {
        return Some(InitialSound::Vowel);
    }

    if matches!(word, ['h', 'e', 'r', 'b', ..])
        && matches!(dialect, Dialect::American | Dialect::Canadian)
    {
        return Some(InitialSound::Vowel);
    }

    if matches!(word, ['u', 'n' | 's', 'i' | 'a' | 'u', ..]) {
        return Some(InitialSound::Consonant);
    }

    if matches!(word, ['u', 'n', ..]) {
        return Some(InitialSound::Vowel);
    }

    if matches!(word, ['u', 'r', 'g', ..]) {
        return Some(InitialSound::Vowel);
    }

    if matches!(word, ['u', 't', 't', ..]) {
        return Some(InitialSound::Vowel);
    }

    if matches!(
        word,
        ['u', 't' | 'r' | 'n', ..] | ['e', 'u', 'r', ..] | ['u', 'w', ..] | ['u', 's', 'e', ..]
    ) {
        return Some(InitialSound::Consonant);
    }

    if matches!(word, ['o', 'n', 'e', 'a' | 'e' | 'i' | 'u', 'l' | 'd', ..]) {
        return Some(InitialSound::Vowel);
    }

    if matches!(word, ['o', 'n', 'e', 'a' | 'e' | 'i' | 'u' | '-' | 's', ..]) {
        return Some(InitialSound::Consonant);
    }

    if matches!(
        word,
        ['s', 'o', 's']
            | ['r', 'z', ..]
            | ['n', 'g', ..]
            | ['n', 'v', ..]
            | ['x', 'b', 'o', 'x']
            | ['h', 'e', 'i', 'r', ..]
            | ['h', 'o', 'n', 'o', 'r', ..]
            | ['h', 'o', 'n', 'e', 's', ..]
    ) {
        return Some(InitialSound::Vowel);
    }

    if matches!(
        word,
        ['j', 'u' | 'o', 'n', ..] | ['j', 'u', 'r', 'a' | 'i' | 'o', ..]
    ) {
        return Some(InitialSound::Consonant);
    }

    if matches!(word, ['x', '-' | '\'' | '.' | 'o' | 's', ..]) {
        return Some(InitialSound::Vowel);
    }

    if is_vowel(word[0]) {
        return Some(InitialSound::Vowel);
    }

    Some(InitialSound::Consonant)
}

/// Whether an all-caps word is read as a word (`NASA`) rather than letter by
/// letter (`HTML`).
fn is_likely_acronym(word: &[char]) -> bool {
    const MIN_LEN: usize = 3;

    let Some(first_chars) = word.get(..MIN_LEN) else {
        return false;
    };
    if !first_chars.iter().copied().all(char::is_alphabetic) {
        return false;
    }
    if word.windows(2).any(|pair| pair == ['V', 'C']) {
        return false;
    }
    let vowels: Vec<bool> = first_chars.iter().map(|&c| is_vowel(c)).collect();
    matches!(
        vowels.as_slice(),
        [false, true, false] | [false, true, true]
    )
}

fn is_article(word: &str) -> bool {
    word.eq_ignore_ascii_case("a") || word.eq_ignore_ascii_case("an")
}

/// Flags an indefinite article that does not match the sound of the next word.
pub(crate) fn lint(text: &str, tokens: &[Token], dialect: Dialect, out: &mut Vec<Finding>) {
    for i in 0..tokens.len().saturating_sub(1) {
        let (first, second) = (tokens[i], tokens[i + 1]);
        if first.kind != TokenKind::Word
            || !matches!(second.kind, TokenKind::Word | TokenKind::Alnum)
            || !gap_is_whitespace(text, first.end, second.start)
        {
            continue;
        }
        let first_text = first.text(text);
        let second_chars: Vec<char> = second
            .text(text)
            .chars()
            .take_while(|c| c.is_alphanumeric())
            .collect();
        if !second_chars.first().is_some_and(|c| c.is_alphabetic()) {
            continue;
        }

        // In "an A student" the A is a grade, not an article.
        if first_text == "A"
            && i > 0
            && tokens[i - 1].kind == TokenKind::Word
            && is_article(tokens[i - 1].text(text))
        {
            continue;
        }

        let is_a = match first_text {
            "a" | "A" => true,
            "an" | "An" => false,
            _ => continue,
        };
        let should_be_a = match starts_with_vowel(&second_chars, dialect) {
            Some(InitialSound::Vowel) => false,
            Some(InitialSound::Consonant) => true,
            Some(InitialSound::Either) | None => continue,
        };
        if is_a == should_be_a {
            continue;
        }

        let capital = first_text.starts_with('A');
        let replacement = match (is_a, capital) {
            (true, false) => "an",
            (true, true) => "An",
            (false, false) => "a",
            (false, true) => "A",
        };
        out.push(Finding {
            start: first.start,
            end: first.end,
            kind: "Miscellaneous",
            message: "Incorrect indefinite article.".to_owned(),
            suggestions: vec![replacement.to_owned()],
            confident: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{check, Dialect, LintConfig};

    fn count(text: &str, dialect: Dialect) -> usize {
        let config = LintConfig {
            dialect,
            ..LintConfig::default()
        };
        check(text, &config)
            .iter()
            .filter(|r| r.kind == "Miscellaneous")
            .count()
    }

    fn american(text: &str) -> usize {
        count(text, Dialect::American)
    }

    fn fixed(text: &str, dialect: Dialect) -> String {
        let config = LintConfig {
            dialect,
            ..LintConfig::default()
        };
        let lints: Vec<_> = check(text, &config)
            .into_iter()
            .filter(|r| r.kind == "Miscellaneous")
            .collect();
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut out: Vec<u16> = Vec::new();
        let mut at = 0usize;
        for lint in &lints {
            out.extend_from_slice(&units[at..lint.from_utf16]);
            out.extend(lint.suggestions[0].encode_utf16());
            at = lint.to_utf16;
        }
        out.extend_from_slice(&units[at..]);
        String::from_utf16(&out).expect("valid text")
    }

    #[test]
    fn detects_html_as_vowel() {
        assert_eq!(american("Here is a HTML document."), 1);
    }

    #[test]
    fn detects_llm_as_vowel() {
        assert_eq!(american("Here is a LLM document."), 1);
    }

    #[test]
    fn detects_llm_hyphen_as_vowel() {
        assert_eq!(american("Here is a LLM-based system."), 1);
    }

    #[test]
    fn detects_euler_as_vowel() {
        assert_eq!(american("This is an Euler brick."), 0);
        assert_eq!(american("The graph has an Eulerian tour."), 0);
    }

    #[test]
    fn capitalized_fourier() {
        assert_eq!(american("Then, perform a Fourier transform."), 0);
    }

    #[test]
    fn once_over() {
        assert_eq!(american("give this a once-over."), 0);
    }

    #[test]
    fn code_between_article_and_word() {
        assert_eq!(american("This is formatted as an `ext4` file system."), 0);
    }

    #[test]
    fn lowercase_vowels_and_consonants() {
        assert_eq!(american("not an error"), 0);
        assert_eq!(american("not a crash"), 0);
        assert_eq!(american("not a error"), 1);
        assert_eq!(american("not an crash"), 1);
    }

    #[test]
    fn uppercase_vowels_and_consonants() {
        assert_eq!(american("not an Error"), 0);
        assert_eq!(american("not a Crash"), 0);
        assert_eq!(american("not a Error"), 1);
        assert_eq!(american("not an Crash"), 1);
    }

    #[test]
    fn disallows_a_interface() {
        assert_eq!(
            american("A interface for an object that can perform linting actions."),
            1
        );
    }

    #[test]
    fn number_after_article() {
        assert_eq!(american("He got a 52% approval rating."), 0);
    }

    #[test]
    fn mp_and_mp3() {
        assert_eq!(american("an MP and an MP3?"), 0);
        assert_eq!(american("an mp3 file"), 0);
        assert_eq!(american("a MP and a MP3?"), 2);
        assert_eq!(american("a mp3 file"), 1);
    }

    #[test]
    fn recognize_acronyms() {
        assert_eq!(american("using a MAC address"), 0);
        assert_eq!(american("a NASA spacecraft"), 0);
        assert_eq!(american("a NAT"), 0);
        assert_eq!(american("a REST API"), 0);
        assert_eq!(american("a LIBERO"), 0);
        assert_eq!(american("a README"), 0);
        assert_eq!(american("a LAN"), 0);
        assert_eq!(american("an RA message"), 0);
        assert_eq!(american("an SI unit"), 0);
        assert_eq!(american("he is an MA of both Oxford and Cambridge"), 0);
        assert_eq!(american("in an FA Cup 6th Round match"), 0);
        assert_eq!(american("a AM transmitter"), 1);
    }

    #[test]
    fn dont_misrecognize_as_acronym() {
        assert_eq!(american("a UPD connection"), 0);
        assert_eq!(american("a UPB device"), 0);
        assert_eq!(american("a UPS or power device"), 0);
        assert_eq!(american("a USB 2.0 port"), 0);
        assert_eq!(american("an HEVC HLS stream"), 0);
    }

    #[test]
    fn partial_initialisms() {
        assert_eq!(american("a udev rule"), 0);
        assert_eq!(american("an mDNS tool"), 0);
        assert_eq!(american("an RFLink device"), 0);
        assert_eq!(american("an FFmpeg-compatible input file"), 0);
    }

    #[test]
    fn words_spelled_against_their_sound() {
        assert_eq!(american("a Honeywell alarm panel"), 0);
        assert_eq!(american("a OneDrive folder"), 0);
        assert_eq!(american("a Ubiquiti UniFi Network application"), 0);
        assert_eq!(american("an honest mistake"), 0);
        assert_eq!(american("an hour"), 0);
        assert_eq!(american("a university"), 0);
        assert_eq!(american("a European trip"), 0);
    }

    #[test]
    fn herb_follows_the_dialect() {
        assert_eq!(count("an herb", Dialect::American), 0);
        assert_eq!(count("a herb", Dialect::British), 0);
        assert_eq!(fixed("an herb", Dialect::Australian), "a herb");
        assert_eq!(fixed("a herb", Dialect::Canadian), "an herb");
    }

    #[test]
    fn either_article_before_sql_led_and_url() {
        assert_eq!(american("a SQL query"), 0);
        assert_eq!(count("an SQL query", Dialect::Australian), 0);
        assert_eq!(american("an LED"), 0);
        assert_eq!(american("a LED"), 0);
        assert_eq!(
            american("I pronounce URL as 'yoo-are-ell' so for me it's 'a URL'"),
            0
        );
        assert_eq!(
            american("But some people pronounce it like 'earl' so for them it's 'an URL'"),
            0
        );
    }

    #[test]
    fn ambiguous_word_does_not_end_the_rule() {
        assert_eq!(american("a URL and a apple"), 1);
    }

    #[test]
    fn grades_are_not_articles() {
        assert_eq!(american("an A-F grading scale"), 0);
        assert_eq!(american("Is 80 an A in Malaysia?"), 0);
    }

    #[test]
    fn npm() {
        assert_eq!(
            american("Why lib/ or light-base/ alone forces an npm bump"),
            0
        );
        assert_eq!(
            fixed(
                "Why lib/ or light-base/ alone forces a npm bump",
                Dialect::American
            ),
            "Why lib/ or light-base/ alone forces an npm bump"
        );
    }

    #[test]
    fn suggestion_keeps_the_capital() {
        assert_eq!(fixed("A apple fell.", Dialect::American), "An apple fell.");
        assert_eq!(
            fixed("An banana fell.", Dialect::American),
            "A banana fell."
        );
        assert_eq!(fixed("not a error", Dialect::American), "not an error");
    }
}
