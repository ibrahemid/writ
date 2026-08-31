use writ_render::{render_markdown_fragment, split_frontmatter};

/// The six inputs `crates/writ-core/tests/prompt_tests.rs` already covers for
/// `strip_frontmatter`, with the body each one leaves. `writ-render` cannot
/// depend on `writ-core`, so the table is hard-coded on both sides and
/// `strip_frontmatter_agrees_with_the_render_split` asserts the mirror image:
/// a divergence fails in both crates.
const SHARED_CASES: &[(&str, &str)] = &[
    (
        "---\ntitle: Test\ntags: [a, b]\n---\nBody text\n",
        "Body text\n",
    ),
    ("---\ntitle: Test\n---\n", ""),
    (
        "---\ntitle: Test\nBody keeps going\n",
        "---\ntitle: Test\nBody keeps going\n",
    ),
    (
        "intro\n---\nnot frontmatter\n---\nrest\n",
        "intro\n---\nnot frontmatter\n---\nrest\n",
    ),
    ("---\r\ntitle: Test\r\n---\r\nBody\r\n", "Body\r\n"),
    ("Body only\n", "Body only\n"),
];

#[test]
fn a_note_with_frontmatter_renders_no_horizontal_rule_and_no_loose_key_value_text() {
    let f = render_markdown_fragment(
        "---\ntitle: Weekly review\ntags: [inbox, draft]\n---\n\n# Heading\n\nbody\n",
    );
    assert!(!f.html.contains("<hr"), "frontmatter rendered a rule");
    assert!(!f.html.contains("title"));
    assert!(!f.html.contains("Weekly review"));
    assert!(!f.html.contains("tags"));
    assert!(!f.html.contains("inbox"));
    assert!(!f.html.contains("draft"));
    assert!(f.html.contains("<h1>Heading</h1>"));
    assert!(f.html.contains("<p>body</p>"));
}

#[test]
fn frontmatter_bytes_survive_a_body_edit_byte_for_byte() {
    let block = "---\n# a comment\ntitle: \"Quoted  value\"   \ntags:\t[x]  \n---\n";
    let text = format!("{block}original body\n");
    let split = split_frontmatter(&text);
    assert_eq!(split.raw, Some(block));
    assert_eq!(split.body, "original body\n");

    let edited = format!("{}{}", split.raw.unwrap(), "rewritten body\n");
    assert_eq!(edited, format!("{block}rewritten body\n"));
    assert_eq!(split_frontmatter(&edited).raw, Some(block));
}

#[test]
fn an_unterminated_block_is_treated_as_body_text() {
    let text = "---\ntitle: Test\nBody keeps going\n";
    let split = split_frontmatter(text);
    assert_eq!(split.raw, None);
    assert_eq!(split.body, text);
}

#[test]
fn a_dash_rule_that_is_not_at_the_start_is_still_a_horizontal_rule() {
    let text = "intro\n\n---\n\nrest\n";
    assert_eq!(split_frontmatter(text).raw, None);
    let f = render_markdown_fragment(text);
    assert!(f.html.contains("<hr"), "a mid-document rule was swallowed");
}

#[test]
fn crlf_line_endings_are_handled() {
    let text = "---\r\ntitle: Test\r\n---\r\nBody\r\n";
    let split = split_frontmatter(text);
    assert_eq!(split.raw, Some("---\r\ntitle: Test\r\n---\r\n"));
    assert_eq!(split.body, "Body\r\n");
}

#[test]
fn an_empty_block_is_recognised_and_hidden() {
    let text = "---\n---\n\nbody\n";
    let split = split_frontmatter(text);
    assert_eq!(split.raw, Some("---\n---\n"));
    assert_eq!(split.body, "\nbody\n");

    let f = render_markdown_fragment(text);
    assert!(!f.html.contains("<hr"), "an empty block rendered a rule");
    assert!(f.html.contains("<p>body</p>"));
}

#[test]
fn split_frontmatter_agrees_with_the_prompt_stripper() {
    for (input, expected_body) in SHARED_CASES {
        assert_eq!(
            split_frontmatter(input).body,
            *expected_body,
            "body mismatch for {input:?}"
        );
    }
}
