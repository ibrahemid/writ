use super::*;

/// Independently computes the UTF-16 range of `needle` in `text` (first
/// occurrence). Uses `encode_utf16` so it can't share a bug with the code
/// under test, which walks `char::len_utf16`.
fn utf16_range_of(text: &str, needle: &str) -> (usize, usize) {
    let byte = text.find(needle).expect("needle present");
    let from = text[..byte].encode_utf16().count();
    let len = needle.encode_utf16().count();
    (from, from + len)
}

fn find_at(results: &[LintResult], from: usize, to: usize) -> Option<&LintResult> {
    results
        .iter()
        .find(|r| r.from_utf16 == from && r.to_utf16 == to)
}

// "recieve" is a stable misspelling Harper corrects to "receive".
const MISSPELLING: &str = "recieve";

#[test]
fn empty_text_yields_no_lints() {
    assert!(check("", &LintConfig::default()).is_empty());
}

#[test]
fn flags_a_plain_misspelling_with_suggestion() {
    let text = "I will recieve it.";
    let results = check(text, &LintConfig::default());
    let (from, to) = utf16_range_of(text, MISSPELLING);
    let lint = find_at(&results, from, to).expect("misspelling flagged");
    assert_eq!(lint.kind, "Spelling");
    assert!(!lint.suggestions.is_empty());
    assert!(lint
        .suggestions
        .iter()
        .any(|s| s.eq_ignore_ascii_case("receive")));
    assert!(lint.confident);
}

#[test]
fn offsets_are_utf16_across_emoji() {
    let text = "😀😀 recieve";
    let results = check(text, &LintConfig::default());
    let (from, to) = utf16_range_of(text, MISSPELLING);
    // Two astral emoji = 4 UTF-16 units + space => offset 5, not the char
    // index (3) and not the byte index (9).
    assert_eq!((from, to), (5, 12));
    assert!(
        find_at(&results, from, to).is_some(),
        "results: {results:?}"
    );
}

#[test]
fn offsets_are_utf16_across_arabic() {
    let text = "مرحبا recieve";
    let results = check(text, &LintConfig::default());
    let (from, to) = utf16_range_of(text, MISSPELLING);
    // Arabic letters are BMP (1 UTF-16 unit) but 2 UTF-8 bytes each.
    assert_eq!(from, 6);
    assert!(
        find_at(&results, from, to).is_some(),
        "results: {results:?}"
    );
}

#[test]
fn offsets_are_utf16_across_cjk() {
    let text = "你好 recieve";
    let results = check(text, &LintConfig::default());
    let (from, to) = utf16_range_of(text, MISSPELLING);
    assert_eq!(from, 3);
    assert!(
        find_at(&results, from, to).is_some(),
        "results: {results:?}"
    );
}

#[test]
fn offsets_correct_with_multibyte_and_inline_code_before_misspelling() {
    // The combination that catches an off-by-one: an astral emoji AND a masked
    // inline-code span before the misspelling, which itself ends the document.
    let text = "😀 `code` recieve";
    let results = check(text, &LintConfig::default());
    let (from, to) = utf16_range_of(text, MISSPELLING);
    let lint = find_at(&results, from, to).expect("misspelling flagged");
    // Ends at the document's total UTF-16 length (prefix off-by-one guard).
    assert_eq!(lint.to_utf16, text.encode_utf16().count());
}

#[test]
fn inline_code_is_masked_and_not_linted() {
    let results = check("`recieve`", &LintConfig::default());
    assert!(
        results.is_empty(),
        "inline code should not be linted: {results:?}"
    );
}

#[test]
fn fenced_code_block_is_masked_and_not_linted() {
    let text = "text\n\n```\nrecieve teh definately\n```\n";
    let results = check(text, &LintConfig::default());
    assert!(
        results.is_empty(),
        "fenced code should not be linted: {results:?}"
    );
}

#[test]
fn url_is_not_linted() {
    let text = "See <http://recieve.example.com/definately> please.";
    let results = check(text, &LintConfig::default());
    // The URL's internal tokens must never surface as spelling lints.
    assert!(
        results.iter().all(|r| r.kind != "Spelling"
            || !text[..].contains("recieve.example")
            || r.from_utf16 >= text.encode_utf16().count()),
        "url tokens should not be linted: {results:?}"
    );
    assert!(
        !results.iter().any(|r| {
            let (from, _) = utf16_range_of(text, "recieve");
            r.from_utf16 == from
        }),
        "url host must not be flagged: {results:?}"
    );
}

#[test]
fn every_reported_lint_carries_a_suggestion() {
    let text = "I recieve teh mesage evry day and definately understnd it.";
    let results = check(text, &LintConfig::default());
    assert!(!results.is_empty());
    for r in &results {
        assert!(
            !r.suggestions.is_empty(),
            "empty-suggestion lint leaked: {r:?}"
        );
        assert!(r.confident);
    }
}

#[test]
fn skips_allcaps_and_mid_word_caps_tokens() {
    let text = "I use SolidJS and the API with useSignal here.";
    let results = check(text, &LintConfig::default());
    let spelling: Vec<_> = results.iter().filter(|r| r.kind == "Spelling").collect();
    assert!(
        spelling.is_empty(),
        "caps identifiers must not be flagged: {spelling:?}"
    );
}

#[test]
fn ignore_list_case_sensitive_stored_form() {
    let text = "I will recieve it.";
    let config = LintConfig {
        ignored_words: vec!["recieve".to_string()],
        ..LintConfig::default()
    };
    let results = check(text, &config);
    assert!(
        results.iter().all(|r| r.kind != "Spelling"),
        "ignored word still flagged: {results:?}"
    );
}

#[test]
fn ignore_list_case_insensitive_fallback() {
    let text = "Recieve it now.";
    let config = LintConfig {
        ignored_words: vec!["recieve".to_string()],
        ..LintConfig::default()
    };
    let results = check(text, &config);
    assert!(
        results.iter().all(|r| r.kind != "Spelling"),
        "case-insensitive ignore failed: {results:?}"
    );
}

#[test]
fn repeated_words_are_flagged_with_a_suggestion() {
    let text = "This is is a test.";
    let results = check(text, &LintConfig::default());
    assert!(
        results.iter().any(|r| r.kind == "Repetition"),
        "repeated word not flagged: {results:?}"
    );
    for r in results.iter().filter(|r| r.kind == "Repetition") {
        assert!(!r.suggestions.is_empty());
        assert!(r.confident);
    }
}

#[test]
fn results_are_sorted_by_start_offset() {
    let text = "I recieve teh mesage evry day.";
    let results = check(text, &LintConfig::default());
    let mut last = 0usize;
    for r in &results {
        assert!(r.from_utf16 >= last, "not sorted: {results:?}");
        last = r.from_utf16;
    }
}

#[test]
#[ignore = "timing probe; run with --release --ignored --nocapture"]
fn timing_100kb_relint() {
    // A ~100KB prose body with periodic misspellings.
    let unit = "The quick brown fox jumps over the lazy dog. I recieve mail daily. ";
    let mut text = String::with_capacity(110_000);
    while text.len() < 100_000 {
        text.push_str(unit);
    }
    let config = LintConfig::default();
    // Warm the curated dictionary and any lazy statics.
    let _ = check(&text, &config);
    let start = std::time::Instant::now();
    let runs = 5;
    for _ in 0..runs {
        let _ = check(&text, &config);
    }
    let per = start.elapsed() / runs;
    println!(
        "writ-lint: warmed re-lint of {} bytes = {:?}/run",
        text.len(),
        per
    );
}

#[test]
fn dialect_from_str_maps_accepted_values_and_falls_back() {
    assert!(matches!(dialect_from_str("american"), Dialect::American));
    assert!(matches!(dialect_from_str("British"), Dialect::British));
    assert!(matches!(dialect_from_str("CANADIAN"), Dialect::Canadian));
    assert!(matches!(
        dialect_from_str("australian"),
        Dialect::Australian
    ));
    assert!(matches!(dialect_from_str("klingon"), Dialect::American));
    assert!(matches!(dialect_from_str(""), Dialect::American));
}
