use writ_plugin::transform::builtins::{
    register_builtins, tidy_whitespace, Dedent, EnsureFinalNewline, FixPunctuationSpacing,
    NormalizeWhitespace, PreparePrompt, SmartToStraightQuotes, TrimLeadingWhitespace,
    TrimTrailingWhitespace,
};
use writ_plugin::transform::{TextTransform, TransformRegistry};

#[test]
fn register_builtins_registers_all_nine_transforms() {
    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).expect("registration must succeed");
    assert_eq!(registry.len(), 9);
    let ids: Vec<String> = registry.list().into_iter().map(|d| d.id).collect();
    assert!(ids.contains(&"trim_leading_whitespace".to_string()));
    assert!(ids.contains(&"trim_trailing_whitespace".to_string()));
    assert!(ids.contains(&"normalize_whitespace".to_string()));
    assert!(ids.contains(&"smart_to_straight_quotes".to_string()));
    assert!(ids.contains(&"dedent".to_string()));
    assert!(ids.contains(&"ensure_final_newline".to_string()));
    assert!(ids.contains(&"fix_punctuation_spacing".to_string()));
    assert!(ids.contains(&"tidy_whitespace".to_string()));
    assert!(ids.contains(&"prepare_prompt".to_string()));
}

#[test]
fn register_builtins_orders_list_alphabetically() {
    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).unwrap();
    let ids: Vec<String> = registry.list().into_iter().map(|d| d.id).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn trim_leading_strips_spaces_from_each_line() {
    let t = TrimLeadingWhitespace;
    assert_eq!(t.apply("  hello\n   world").unwrap(), "hello\nworld");
}

#[test]
fn trim_leading_strips_tabs() {
    let t = TrimLeadingWhitespace;
    assert_eq!(t.apply("\thello\n\t\tworld").unwrap(), "hello\nworld");
}

#[test]
fn trim_leading_preserves_trailing_whitespace() {
    let t = TrimLeadingWhitespace;
    assert_eq!(t.apply("  hello  ").unwrap(), "hello  ");
}

#[test]
fn trim_leading_preserves_blank_lines() {
    let t = TrimLeadingWhitespace;
    assert_eq!(t.apply("a\n\n  b").unwrap(), "a\n\nb");
}

#[test]
fn trim_leading_preserves_crlf_line_endings() {
    let t = TrimLeadingWhitespace;
    assert_eq!(t.apply("  a\r\n  b\r\n").unwrap(), "a\r\nb\r\n");
}

#[test]
fn trim_leading_handles_empty_input() {
    let t = TrimLeadingWhitespace;
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn trim_leading_handles_only_whitespace() {
    let t = TrimLeadingWhitespace;
    assert_eq!(t.apply("   \n\t").unwrap(), "\n");
}

#[test]
fn trim_leading_is_idempotent() {
    let t = TrimLeadingWhitespace;
    let once = t.apply("   hello\n\t world").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn normalize_collapses_internal_runs_of_spaces() {
    let t = NormalizeWhitespace;
    assert_eq!(t.apply("a    b").unwrap(), "a b");
}

#[test]
fn normalize_collapses_mixed_tabs_and_spaces() {
    let t = NormalizeWhitespace;
    assert_eq!(t.apply("a \t \t b").unwrap(), "a b");
}

#[test]
fn normalize_preserves_leading_whitespace() {
    let t = NormalizeWhitespace;
    assert_eq!(t.apply("    a    b").unwrap(), "    a b");
}

#[test]
fn normalize_collapses_trailing_to_single_space() {
    let t = NormalizeWhitespace;
    assert_eq!(t.apply("a   ").unwrap(), "a ");
}

#[test]
fn normalize_preserves_blank_lines() {
    let t = NormalizeWhitespace;
    assert_eq!(t.apply("a\n\n\nb").unwrap(), "a\n\n\nb");
}

#[test]
fn normalize_handles_crlf() {
    let t = NormalizeWhitespace;
    assert_eq!(t.apply("a   b\r\nc   d").unwrap(), "a b\r\nc d");
}

#[test]
fn normalize_empty_input() {
    let t = NormalizeWhitespace;
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn normalize_is_idempotent() {
    let t = NormalizeWhitespace;
    let once = t.apply("a   b   c").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn smart_quotes_converts_curly_singles() {
    let t = SmartToStraightQuotes;
    assert_eq!(t.apply("don\u{2019}t").unwrap(), "don't");
    assert_eq!(t.apply("\u{2018}quoted\u{2019}").unwrap(), "'quoted'");
}

#[test]
fn smart_quotes_converts_curly_doubles() {
    let t = SmartToStraightQuotes;
    assert_eq!(t.apply("\u{201C}hello\u{201D}").unwrap(), "\"hello\"");
}

#[test]
fn smart_quotes_converts_low_quotes() {
    let t = SmartToStraightQuotes;
    assert_eq!(
        t.apply("\u{201A}a\u{201B} \u{201E}b\u{201F}").unwrap(),
        "'a' \"b\""
    );
}

#[test]
fn smart_quotes_preserves_emoji_and_other_unicode() {
    let t = SmartToStraightQuotes;
    assert_eq!(t.apply("hi 👋 \u{2014} bye").unwrap(), "hi 👋 \u{2014} bye");
}

#[test]
fn smart_quotes_no_op_on_already_straight() {
    let t = SmartToStraightQuotes;
    assert_eq!(t.apply("'a' \"b\"").unwrap(), "'a' \"b\"");
}

#[test]
fn smart_quotes_empty_input() {
    let t = SmartToStraightQuotes;
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn smart_quotes_is_idempotent() {
    let t = SmartToStraightQuotes;
    let once = t.apply("\u{2018}a\u{2019} \u{201C}b\u{201D}").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn dedent_removes_common_four_space_prefix() {
    let t = Dedent;
    assert_eq!(t.apply("    a\n    b\n    c").unwrap(), "a\nb\nc");
}

#[test]
fn dedent_removes_common_tab_prefix() {
    let t = Dedent;
    assert_eq!(t.apply("\t\ta\n\t\tb").unwrap(), "a\nb");
}

#[test]
fn dedent_uses_shortest_common_prefix() {
    let t = Dedent;
    assert_eq!(t.apply("    a\n      b\n    c").unwrap(), "a\n  b\nc");
}

#[test]
fn dedent_ignores_blank_lines_when_computing_prefix() {
    let t = Dedent;
    assert_eq!(t.apply("    a\n\n    b").unwrap(), "a\n\nb");
}

#[test]
fn dedent_preserves_line_endings() {
    let t = Dedent;
    assert_eq!(t.apply("    a\r\n    b\r\n").unwrap(), "a\r\nb\r\n");
}

#[test]
fn dedent_no_common_prefix_returns_unchanged() {
    let t = Dedent;
    assert_eq!(t.apply("a\n  b").unwrap(), "a\n  b");
}

#[test]
fn dedent_mixed_tabs_and_spaces_takes_byte_prefix() {
    let t = Dedent;
    assert_eq!(t.apply("\t a\n\t b\n\t c").unwrap(), "a\nb\nc");
    assert_eq!(t.apply("\ta\n a").unwrap(), "\ta\n a");
}

#[test]
fn dedent_empty_input() {
    let t = Dedent;
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn dedent_all_blank_lines() {
    let t = Dedent;
    assert_eq!(t.apply("   \n\t\n").unwrap(), "   \n\t\n");
}

#[test]
fn dedent_is_idempotent_after_first_application() {
    let t = Dedent;
    let once = t.apply("    a\n      b\n    c").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn trim_trailing_strips_spaces_and_tabs_from_each_line() {
    let t = TrimTrailingWhitespace;
    assert_eq!(t.apply("hello   \nworld\t").unwrap(), "hello\nworld");
}

#[test]
fn trim_trailing_preserves_leading_whitespace() {
    let t = TrimTrailingWhitespace;
    assert_eq!(t.apply("    hello   ").unwrap(), "    hello");
}

#[test]
fn trim_trailing_blanks_whitespace_only_lines() {
    let t = TrimTrailingWhitespace;
    assert_eq!(t.apply("a\n   \nb").unwrap(), "a\n\nb");
}

#[test]
fn trim_trailing_preserves_crlf_line_endings() {
    let t = TrimTrailingWhitespace;
    assert_eq!(t.apply("a  \r\nb\t\r\n").unwrap(), "a\r\nb\r\n");
}

#[test]
fn trim_trailing_handles_empty_input() {
    let t = TrimTrailingWhitespace;
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn trim_trailing_is_idempotent() {
    let t = TrimTrailingWhitespace;
    let once = t.apply("a  \nb\t \nc ").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn ensure_final_newline_adds_missing_newline() {
    let t = EnsureFinalNewline;
    assert_eq!(t.apply("abc").unwrap(), "abc\n");
}

#[test]
fn ensure_final_newline_keeps_single_existing_newline() {
    let t = EnsureFinalNewline;
    assert_eq!(t.apply("abc\n").unwrap(), "abc\n");
}

#[test]
fn ensure_final_newline_collapses_multiple_trailing_newlines() {
    let t = EnsureFinalNewline;
    assert_eq!(t.apply("abc\n\n\n").unwrap(), "abc\n");
}

#[test]
fn ensure_final_newline_uses_crlf_when_input_is_crlf() {
    let t = EnsureFinalNewline;
    assert_eq!(t.apply("a\r\nb").unwrap(), "a\r\nb\r\n");
    assert_eq!(t.apply("a\r\nb\r\n\r\n").unwrap(), "a\r\nb\r\n");
}

#[test]
fn ensure_final_newline_empty_input_stays_empty() {
    let t = EnsureFinalNewline;
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn ensure_final_newline_only_newlines_collapse_to_empty() {
    let t = EnsureFinalNewline;
    assert_eq!(t.apply("\n\n").unwrap(), "");
}

#[test]
fn ensure_final_newline_is_idempotent() {
    let t = EnsureFinalNewline;
    let once = t.apply("abc\n\n").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn fix_punctuation_removes_space_before_comma_and_period() {
    let t = FixPunctuationSpacing;
    assert_eq!(t.apply("Hello , world .").unwrap(), "Hello, world.");
}

#[test]
fn fix_punctuation_removes_space_before_semicolon_colon_bang_question() {
    let t = FixPunctuationSpacing;
    assert_eq!(t.apply("a ; b : c ! d ?").unwrap(), "a; b: c! d?");
}

#[test]
fn fix_punctuation_preserves_decimals() {
    let t = FixPunctuationSpacing;
    assert_eq!(t.apply("pi is 3.14 today").unwrap(), "pi is 3.14 today");
}

#[test]
fn fix_punctuation_preserves_urls() {
    let t = FixPunctuationSpacing;
    assert_eq!(
        t.apply("see http://example.com for more").unwrap(),
        "see http://example.com for more"
    );
}

#[test]
fn fix_punctuation_collapses_space_before_trailing_url_period() {
    let t = FixPunctuationSpacing;
    assert_eq!(
        t.apply("see http://example.com .").unwrap(),
        "see http://example.com."
    );
}

#[test]
fn fix_punctuation_does_not_split_ellipsis() {
    let t = FixPunctuationSpacing;
    assert_eq!(t.apply("wait ... really").unwrap(), "wait... really");
    assert_eq!(t.apply("a...b").unwrap(), "a...b");
}

#[test]
fn fix_punctuation_leaves_glued_punctuation_untouched() {
    let t = FixPunctuationSpacing;
    assert_eq!(t.apply("a ,b").unwrap(), "a ,b");
}

#[test]
fn fix_punctuation_preserves_crlf() {
    let t = FixPunctuationSpacing;
    assert_eq!(t.apply("a , b\r\nc .\r\n").unwrap(), "a, b\r\nc.\r\n");
}

#[test]
fn fix_punctuation_empty_input() {
    let t = FixPunctuationSpacing;
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn fix_punctuation_is_idempotent() {
    let t = FixPunctuationSpacing;
    let once = t.apply("Hello , world . Bye ;").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn tidy_whitespace_runs_the_full_pipeline() {
    let t = tidy_whitespace();
    assert_eq!(t.id(), "tidy_whitespace");
    let input = "    foo   bar   \n    baz\t\n\n\n";
    assert_eq!(t.apply(input).unwrap(), "foo bar\nbaz\n");
}

#[test]
fn tidy_whitespace_adds_final_newline() {
    let t = tidy_whitespace();
    assert_eq!(t.apply("hello   world").unwrap(), "hello world\n");
}

#[test]
fn tidy_whitespace_empty_input_stays_empty() {
    let t = tidy_whitespace();
    assert_eq!(t.apply("").unwrap(), "");
}

#[test]
fn tidy_whitespace_is_idempotent_after_first_application() {
    let t = tidy_whitespace();
    let once = t.apply("    foo   bar   \n    baz\t\n\n").unwrap();
    let twice = t.apply(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn prepare_prompt_strips_frontmatter_and_comments() {
    let t = PreparePrompt;
    let input = "---\ndraft: true\n---\n<!-- note to self -->\nThe prompt body.  \n\n";
    assert_eq!(t.apply(input).unwrap(), "\nThe prompt body.\n");
}

#[test]
fn prepare_prompt_preserves_comments_inside_fences() {
    let t = PreparePrompt;
    let input = "```html\n<!-- keep -->\n```\n";
    assert_eq!(t.apply(input).unwrap(), "```html\n<!-- keep -->\n```\n");
}

#[test]
fn prepare_prompt_does_not_mutate_plain_text_beyond_tail() {
    let t = PreparePrompt;
    assert_eq!(t.apply("plain prompt text").unwrap(), "plain prompt text\n");
}

#[test]
fn prepare_prompt_metadata_is_palette_ready() {
    let t = PreparePrompt;
    assert_eq!(t.id(), "prepare_prompt");
    assert_eq!(t.metadata().label, "Prepare as Prompt");
    assert!(!t.metadata().description.is_empty());
}
