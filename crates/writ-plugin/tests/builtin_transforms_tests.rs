use writ_plugin::transform::builtins::{
    register_builtins, Dedent, NormalizeWhitespace, SmartToStraightQuotes, TrimLeadingWhitespace,
};
use writ_plugin::transform::{TextTransform, TransformRegistry};

#[test]
fn register_builtins_registers_all_four_transforms() {
    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).expect("registration must succeed");
    assert_eq!(registry.len(), 4);
    let ids: Vec<String> = registry.list().into_iter().map(|d| d.id).collect();
    assert!(ids.contains(&"trim_leading_whitespace".to_string()));
    assert!(ids.contains(&"normalize_whitespace".to_string()));
    assert!(ids.contains(&"smart_to_straight_quotes".to_string()));
    assert!(ids.contains(&"dedent".to_string()));
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
