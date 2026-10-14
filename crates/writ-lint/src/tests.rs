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

// "recieve" is a stable misspelling the dictionaries correct to "receive",
// among other candidates.
const MISSPELLING: &str = "recieve";

fn config_for(dialect: Dialect) -> LintConfig {
    LintConfig {
        dialect,
        ..LintConfig::default()
    }
}

fn spelling_at<'a>(results: &'a [LintResult], text: &str, word: &str) -> Option<&'a LintResult> {
    let (from, to) = utf16_range_of(text, word);
    results
        .iter()
        .find(|r| r.kind == "Spelling" && r.from_utf16 == from && r.to_utf16 == to)
}

/// Applies the first suggestion of every lint of `kind`, as Fix all does.
fn apply_first(text: &str, kind: &str, config: &LintConfig) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::new();
    let mut at = 0usize;
    for lint in check(text, config).iter().filter(|r| r.kind == kind) {
        out.extend_from_slice(&units[at..lint.from_utf16]);
        out.extend(lint.suggestions[0].encode_utf16());
        at = lint.to_utf16;
    }
    out.extend_from_slice(&units[at..]);
    String::from_utf16(&out).expect("valid text")
}

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
fn autolink_url_is_not_linted() {
    let text = "See <http://recieve.example.com/definately> please.";
    let (from, _) = utf16_range_of(text, "recieve");
    let results = check(text, &LintConfig::default());
    assert!(
        !results.iter().any(|r| r.from_utf16 == from),
        "autolink url host must not be flagged: {results:?}"
    );
}

#[test]
fn bare_url_is_not_linted() {
    let text = "Visit http://recieve.example.com/definately today.";
    let (from, _) = utf16_range_of(text, "recieve");
    let results = check(text, &LintConfig::default());
    assert!(
        !results.iter().any(|r| r.from_utf16 == from),
        "bare url host must not be flagged: {results:?}"
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

#[test]
fn every_bundled_dictionary_parses() {
    for dialect in [
        Dialect::American,
        Dialect::British,
        Dialect::Canadian,
        Dialect::Australian,
    ] {
        let dictionary = spelling::dictionary(dialect).expect("bundled dictionary parses");
        assert!(dictionary.check("receive"), "{dialect:?}");
        assert!(!dictionary.check(MISSPELLING), "{dialect:?}");
    }
}

#[test]
fn spelling_follows_the_chosen_english() {
    let text = "The colour of the sky.";
    assert!(
        spelling_at(&check(text, &config_for(Dialect::American)), text, "colour").is_some(),
        "American English flags colour"
    );
    for dialect in [Dialect::British, Dialect::Canadian, Dialect::Australian] {
        let results = check(text, &config_for(dialect));
        assert!(
            spelling_at(&results, text, "colour").is_none(),
            "{dialect:?} accepts colour: {results:?}"
        );
    }
    let text = "The color of the sky.";
    assert!(spelling_at(&check(text, &config_for(Dialect::British)), text, "color").is_some());
    assert!(spelling_at(&check(text, &config_for(Dialect::American)), text, "color").is_none());
}

const DIALECTS: [Dialect; 4] = [
    Dialect::American,
    Dialect::British,
    Dialect::Canadian,
    Dialect::Australian,
];

#[test]
fn developer_words_harper_accepted_are_not_flagged() {
    // On SCOWL alone this sentence had five spelling lints, and Tauri a
    // confident fix to Lauri.
    let text = "The Tauri frontend calls the backend async on localhost.";
    for dialect in DIALECTS {
        let results = check(text, &config_for(dialect));
        assert!(results.is_empty(), "{dialect:?}: {results:?}");
    }
}

#[test]
fn inflected_forms_harper_accepted_are_not_flagged() {
    for text in [
        "Two configs, three backends, the frontend's cache and Tauri's window.",
        "The frontend’s cache and Tauri’s window.",
    ] {
        for dialect in DIALECTS {
            let results = check(text, &config_for(dialect));
            assert!(results.is_empty(), "{dialect:?}: {results:?}");
        }
    }
}

#[test]
fn words_harper_flagged_stay_flagged() {
    let text = "Clear the todos.";
    for dialect in DIALECTS {
        let results = check(text, &config_for(dialect));
        assert!(
            spelling_at(&results, text, "todos").is_some(),
            "{dialect:?}: {results:?}"
        );
    }
}

#[test]
fn harper_words_join_only_the_variants_they_are_listed_for() {
    let american = "Its favorability is multicolor.";
    let british = "He spoke humourlessly.";
    let results = check(american, &config_for(Dialect::American));
    assert!(results.is_empty(), "{results:?}");
    let results = check(american, &config_for(Dialect::British));
    for word in ["favorability", "multicolor"] {
        assert!(
            spelling_at(&results, american, word).is_some(),
            "{word}: {results:?}"
        );
    }
    let results = check(british, &config_for(Dialect::British));
    assert!(results.is_empty(), "{results:?}");
    let results = check(british, &config_for(Dialect::American));
    assert!(
        spelling_at(&results, british, "humourlessly").is_some(),
        "{results:?}"
    );
}

#[test]
fn every_harper_word_is_in_the_dictionary_of_its_variants() {
    for dialect in DIALECTS {
        let dictionary = spelling::dictionary(dialect).expect("bundled dictionary parses");
        let words: Vec<&str> = spelling::harper_words(dialect).collect();
        assert!(words.len() > 20_000, "{dialect:?}: {}", words.len());
        let missing: Vec<&&str> = words.iter().filter(|w| !dictionary.check(w)).collect();
        assert!(missing.is_empty(), "{dialect:?}: {missing:?}");
    }
}

#[test]
fn arabic_cyrillic_and_cjk_words_produce_no_spelling_lint() {
    for text in [
        "مرحبا بالعالم، هذا نص عربي قصير للتجربة.",
        "Привет, мир. Это короткий русский текст.",
        "你好，世界。这是一段中文文本。",
        "こんにちは、世界。日本語の文章です。",
        "안녕하세요 세계. 한국어 문장입니다.",
    ] {
        let results = check(text, &LintConfig::default());
        assert!(results.is_empty(), "{text}: {results:?}");
    }
}

#[test]
fn a_word_repeated_in_arabic_is_not_flagged() {
    let results = check("هذا جميل جدا جدا.", &LintConfig::default());
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn accented_and_mixed_script_words_are_not_spell_checked() {
    let text = "A café in Zürich serves crème brûlée.";
    let results = check(text, &LintConfig::default());
    assert!(results.iter().all(|r| r.kind != "Spelling"), "{results:?}");
}

#[test]
fn yaml_front_matter_is_not_linted() {
    let text = "---\ntitle: Recieve teh plans\ntags: [definately]\n---\n\nI recieve it.\n";
    let results = check(text, &LintConfig::default());
    let body = text.rfind(MISSPELLING).expect("body misspelling");
    let body_from = text[..body].encode_utf16().count();
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].from_utf16, body_from);
}

#[test]
fn toml_front_matter_is_not_linted() {
    let text = "+++\ntitle = \"Recieve teh plans\"\n+++\n\nAll good here.\n";
    let results = check(text, &LintConfig::default());
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn link_text_is_checked_and_link_targets_are_not() {
    let text = "Read [the recieve guide](https://definately.example.com/teh) now.";
    let results = check(text, &LintConfig::default());
    assert!(
        spelling_at(&results, text, MISSPELLING).is_some(),
        "{results:?}"
    );
    assert_eq!(results.len(), 1, "{results:?}");
}

#[test]
fn email_and_path_tokens_are_not_linted() {
    let text = "Mail recieve.team@exampel.com or open src/definately/teh.rs today.";
    let results = check(text, &LintConfig::default());
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn wikilink_target_is_not_linted_and_its_label_is() {
    let text = "See [[recieve plans]] today.";
    let results = check(text, &LintConfig::default());
    assert!(results.is_empty(), "wikilink target flagged: {results:?}");

    let text = "See [[plans|recieve this]] today.";
    let results = check(text, &LintConfig::default());
    assert!(
        spelling_at(&results, text, MISSPELLING).is_some(),
        "{results:?}"
    );
}

#[test]
fn html_and_image_alt_text_are_not_linted() {
    let text = "<div class=\"recieve\"></div>\n\n![teh definately picture](a.png)\n";
    let results = check(text, &LintConfig::default());
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn indented_code_block_is_not_linted() {
    let text = "Words here.\n\n    recieve teh definately\n";
    let results = check(text, &LintConfig::default());
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn spelling_with_several_suggestions_is_not_confident() {
    // Before this engine every lint was confident, so this assertion failed.
    let text = "I will recieve it.";
    let results = check(text, &LintConfig::default());
    let lint = spelling_at(&results, text, MISSPELLING).expect("flagged");
    assert!(lint.suggestions.len() > 1, "{lint:?}");
    assert!(!lint.confident, "{lint:?}");
}

#[test]
fn single_suggestion_one_edit_away_is_confident() {
    // A transposition, a substitution and an insertion.
    for (text, word, fix) in [
        ("We acheive it.", "acheive", "achieve"),
        ("It is definately so.", "definately", "definitely"),
        ("We accomodate them.", "accomodate", "accommodate"),
    ] {
        let results = check(text, &LintConfig::default());
        let lint = spelling_at(&results, text, word).expect("flagged");
        assert_eq!(lint.suggestions, vec![fix.to_string()], "{lint:?}");
        assert!(lint.confident, "{lint:?}");
        assert_eq!(lint.message, format!("Did you mean `{fix}`?"));
    }
}

#[test]
fn single_suggestion_more_than_one_edit_away_is_not_confident() {
    let text = "It is neccesarry.";
    let results = check(text, &LintConfig::default());
    let lint = spelling_at(&results, text, "neccesarry").expect("flagged");
    assert_eq!(lint.suggestions, vec!["necessary".to_string()]);
    assert_eq!(lint.suggestions.len(), 1, "{lint:?}");
    assert!(!lint.confident, "{lint:?}");
}

#[test]
fn spelling_carries_at_most_three_suggestions() {
    let text = "Fix teh thing.";
    let results = check(text, &LintConfig::default());
    let lint = spelling_at(&results, text, "teh").expect("flagged");
    assert_eq!(lint.suggestions.len(), 3, "{lint:?}");
    assert_eq!(lint.suggestions[0], "the");
    assert!(!lint.confident);
}

#[test]
fn a_misspelling_with_no_edit_candidate_still_gets_a_suggestion() {
    let text = "See you tommorow.";
    let results = check(text, &LintConfig::default());
    let lint = spelling_at(&results, text, "tommorow").expect("flagged");
    assert_eq!(lint.suggestions[0], "tomorrow");
}

#[test]
fn capitalised_misspelling_gets_capitalised_suggestions() {
    let text = "Recieve it.";
    let results = check(text, &LintConfig::default());
    let lint = spelling_at(&results, text, "Recieve").expect("flagged");
    assert_eq!(lint.suggestions[0], "Receive");
}

#[test]
fn contractions_with_either_apostrophe_are_words() {
    let text = "I don't know and I don\u{2019}t care, it's fine.";
    let results = check(text, &LintConfig::default());
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn one_edit_apart_counts_each_kind_of_edit_once() {
    assert!(spelling::is_one_edit_apart("cat", "bat"));
    assert!(spelling::is_one_edit_apart("recieve", "receive"));
    assert!(spelling::is_one_edit_apart("acomodate", "accomodate"));
    assert!(spelling::is_one_edit_apart("untill", "until"));
    assert!(!spelling::is_one_edit_apart("same", "same"));
    assert!(!spelling::is_one_edit_apart("cat", "dog"));
    assert!(!spelling::is_one_edit_apart("abcd", "badc"));
    assert!(!spelling::is_one_edit_apart("tommorow", "tomorrow"));
    assert!(!spelling::is_one_edit_apart("ab", "abcd"));
}

#[test]
fn repeated_word_fix_keeps_one_copy() {
    let config = LintConfig::default();
    assert_eq!(
        apply_first("This is is a test.", "Repetition", &config),
        "This is a test."
    );
    assert_eq!(
        apply_first("This is a a test.", "Repetition", &config),
        "This is a test."
    );
    assert_eq!(
        apply_first("And and this is also a test.", "Repetition", &config),
        "And this is also a test."
    );
    assert_eq!(
        apply_first("Look at the project on\non GitHub.", "Repetition", &config),
        "Look at the project on GitHub."
    );
}

#[test]
fn repeated_word_lints_never_overlap() {
    let text = "the the the end";
    let results: Vec<_> = check(text, &LintConfig::default())
        .into_iter()
        .filter(|r| r.kind == "Repetition")
        .collect();
    assert_eq!(results.len(), 1, "{results:?}");
}

#[test]
fn legitimate_repeats_are_not_flagged() {
    for text in [
        "She had had enough.",
        "He said that that was fine.",
        "ha ha",
        "Ha ha ha",
        "foo-foo foo bar bar-bar",
        "The driver-facing camera is logged if you opt-in in settings.",
        "the `code` the",
        "One sentence ends here. Here it starts again.",
    ] {
        let results: Vec<_> = check(text, &LintConfig::default())
            .into_iter()
            .filter(|r| r.kind == "Repetition")
            .collect();
        assert!(results.is_empty(), "{text}: {results:?}");
    }
}

#[test]
fn repeated_words_across_paragraphs_are_not_flagged() {
    let text = "The end of one is\n\nis the start of another.";
    let results: Vec<_> = check(text, &LintConfig::default())
        .into_iter()
        .filter(|r| r.kind == "Repetition")
        .collect();
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn repeated_spaces_are_flagged_and_fixed() {
    let text = "Two  spaces and   three.";
    let config = LintConfig::default();
    let results: Vec<_> = check(text, &config)
        .into_iter()
        .filter(|r| r.kind == "Formatting")
        .collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results.iter().all(|r| r.confident));
    assert_eq!(
        results[1].message,
        "There are 3 spaces where there should be only one."
    );
    assert_eq!(
        apply_first(text, "Formatting", &config),
        "Two spaces and three."
    );
}

#[test]
fn space_before_a_period_is_removed() {
    let text = "There is a space at the end of this sentence .";
    let config = LintConfig::default();
    let results: Vec<_> = check(text, &config)
        .into_iter()
        .filter(|r| r.kind == "Formatting")
        .collect();
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(
        results[0].message,
        "Unnecessary space at the end of the sentence."
    );
    assert_eq!(
        apply_first(text, "Formatting", &config),
        "There is a space at the end of this sentence."
    );
}

#[test]
fn spacing_conventions_are_not_flagged() {
    for text in [
        "This is a short sentence.  This is another short sentence.",
        "He said \"stop.\"  Then he left.",
        "A line with a hard break  \nand the next line.",
        "| a   | b   |\n| --- | --- |\n| one | two |\n",
        "Wait for it ...",
        "Run `make` .",
        "    indented  code\n",
        "Trailing spaces   ",
    ] {
        let results: Vec<_> = check(text, &LintConfig::default())
            .into_iter()
            .filter(|r| r.kind == "Formatting")
            .collect();
        assert!(results.is_empty(), "{text:?}: {results:?}");
    }
}

#[test]
fn spaces_before_inline_markup_are_flagged() {
    let text = "Some  **bold** text.";
    let config = LintConfig::default();
    assert_eq!(
        apply_first(text, "Formatting", &config),
        "Some **bold** text."
    );
}

#[test]
fn wrong_apostrophes_are_fixed() {
    let config = LintConfig::default();
    for (text, fixed) in [
        (
            "It's better if you don;t type like this.",
            "It's better if you don't type like this.",
        ),
        ("Don;t type like this.", "Don't type like this."),
        ("DON;T TRY THIS AT HOME.", "DON'T TRY THIS AT HOME."),
        (
            "Well I;ll be a monkey;s uncle!",
            "Well I'll be a monkey's uncle!",
        ),
        ("It\u{00B4}s here.", "It's here."),
        ("We`re here.", "We're here."),
    ] {
        assert_eq!(apply_first(text, "Typo", &config), fixed, "{text}");
    }
}

#[test]
fn wrong_apostrophe_lint_is_the_only_lint_on_its_word() {
    let text = "You don;t say.";
    let results = check(text, &LintConfig::default());
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].kind, "Typo");
    assert!(results[0].confident);
    assert_eq!(results[0].message, "Did you mean `don't`?");
}

#[test]
fn semicolons_between_words_are_not_apostrophes() {
    for text in [
        "I don't doubt it.",
        "First this; then that.",
        "red;green;blue",
        "items;stuff",
    ] {
        let results: Vec<_> = check(text, &LintConfig::default())
            .into_iter()
            .filter(|r| r.kind == "Typo")
            .collect();
        assert!(results.is_empty(), "{text}: {results:?}");
    }
}

#[test]
fn a_an_is_flagged_through_check_and_is_confident() {
    let text = "It was a error.";
    let results = check(text, &LintConfig::default());
    let lint = results
        .iter()
        .find(|r| r.kind == "Miscellaneous")
        .expect("a/an flagged");
    assert_eq!(lint.suggestions, vec!["an".to_string()]);
    assert!(lint.confident);
    assert_eq!(lint.message, "Incorrect indefinite article.");
}

#[test]
fn mechanical_rules_are_always_confident() {
    let text = "This is is a  test with a error and don;t stop .";
    let results = check(text, &LintConfig::default());
    let kinds: Vec<&str> = results.iter().map(|r| r.kind.as_str()).collect();
    for kind in ["Repetition", "Formatting", "Miscellaneous", "Typo"] {
        assert!(kinds.contains(&kind), "{kind} missing: {results:?}");
    }
    assert!(results
        .iter()
        .filter(|r| r.kind != "Spelling")
        .all(|r| r.confident));
}

#[test]
fn html_entities_are_not_words() {
    let text = "Fish&nbsp;and&amp;chips &#169; here.";
    let results = check(text, &LintConfig::default());
    assert!(results.is_empty(), "{results:?}");
}

#[test]
fn ngram_search_runs_only_within_its_budget() {
    let dictionary = spelling::dictionary(Dialect::American).expect("parses");

    // Edit-based candidates need no budget.
    let mut spent = 0usize;
    let found = spelling::suggestions_for(dictionary, Dialect::American, "acheive", &mut spent)
        .expect("edit candidates");
    assert_eq!(found, vec!["achieve".to_string()]);

    // A word only the ngram search can correct waits for budget, uncached.
    let mut none_left = 0usize;
    assert!(spelling::suggestions_for(
        dictionary,
        Dialect::American,
        "embarasment",
        &mut none_left
    )
    .is_none());
    let mut one_left = 1usize;
    let found =
        spelling::suggestions_for(dictionary, Dialect::American, "embarasment", &mut one_left)
            .expect("ngram candidates");
    assert_eq!(found[0], "embarrassment");
    assert_eq!(one_left, 0);
}
