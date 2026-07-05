use std::collections::HashMap;

use writ_core::prompt::{estimate_tokens, fill_placeholders, scan_placeholders, strip_for_prompt};

// --- estimate_tokens ---

const PROSE: &str = "The status bar answers a single question for anyone drafting a prompt: \
does this text fit the context window I am paying for? An estimate within a third of the \
real figure answers that question for every practical budget, because budgets come in powers \
of two and the gaps between them are enormous. Nobody chooses between models over a few \
dozen tokens; they choose over thousands.";

const CODE: &str = r#"fn parse_ident(s: &str) -> Option<(usize, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    let mut end = first.len_utf8();
    for (idx, c) in chars {
        if c.is_alphanumeric() || c == '_' {
            end = idx + c.len_utf8();
        } else {
            break;
        }
    }
    Some((end, &s[..end]))
}
"#;

const MARKDOWN: &str = "# Release checklist\n\n- Run the **full** test suite before tagging.\n\
- Update the [changelog](https://example.com/changelog) with every user-facing change.\n\
- Verify the installer on a clean machine, not a dev box.\n";

#[test]
fn empty_input_estimates_zero() {
    assert_eq!(estimate_tokens(""), 0);
}

#[test]
fn whitespace_only_estimates_zero() {
    assert_eq!(estimate_tokens("   \n\t  \n"), 0);
}

#[test]
fn single_word_estimates_at_least_one() {
    assert!(estimate_tokens("a") >= 1);
    assert!(estimate_tokens("hello") >= 1);
}

#[test]
fn prose_lands_within_band_of_word_rule() {
    let words = PROSE.split_whitespace().count() as f64;
    let reference = words * 4.0 / 3.0;
    let est = estimate_tokens(PROSE) as f64;
    assert!(
        est >= reference * 0.7 && est <= reference * 1.3,
        "prose estimate {est} outside ±30% of word-rule reference {reference}"
    );
}

#[test]
fn prose_lands_within_band_of_char_rule() {
    let chars = PROSE.chars().count() as f64;
    let reference = chars / 4.0;
    let est = estimate_tokens(PROSE) as f64;
    assert!(
        est >= reference * 0.7 && est <= reference * 1.3,
        "prose estimate {est} outside ±30% of char-rule reference {reference}"
    );
}

#[test]
fn code_lands_within_band_of_dense_char_rule() {
    let chars = CODE.chars().count() as f64;
    let reference = chars / 3.5;
    let est = estimate_tokens(CODE) as f64;
    assert!(
        est >= reference * 0.7 && est <= reference * 1.3,
        "code estimate {est} outside ±30% of dense char-rule reference {reference}"
    );
}

#[test]
fn markdown_lands_within_band_of_char_rule() {
    let chars = MARKDOWN.chars().count() as f64;
    let reference = chars / 4.0;
    let est = estimate_tokens(MARKDOWN) as f64;
    assert!(
        est >= reference * 0.7 && est <= reference * 1.3,
        "markdown estimate {est} outside ±30% of char-rule reference {reference}"
    );
}

#[test]
fn mixed_prose_and_code_lands_between_its_parts() {
    let mixed = format!("{PROSE}\n\n```rust\n{CODE}```\n\n{MARKDOWN}");
    let est = estimate_tokens(&mixed);
    let parts = estimate_tokens(PROSE) + estimate_tokens(CODE) + estimate_tokens(MARKDOWN);
    let est_f = est as f64;
    let parts_f = parts as f64;
    assert!(
        est_f >= parts_f * 0.9 && est_f <= parts_f * 1.1,
        "mixed estimate {est} should track the sum of its parts {parts}"
    );
}

#[test]
fn estimate_is_monotonic_in_length() {
    let short = estimate_tokens(PROSE);
    let long = estimate_tokens(&format!("{PROSE} {PROSE}"));
    assert!(long > short);
}

// --- strip_for_prompt ---

#[test]
fn plain_text_passes_through_with_final_newline() {
    assert_eq!(strip_for_prompt("hello world"), "hello world\n");
}

#[test]
fn empty_input_stays_empty() {
    assert_eq!(strip_for_prompt(""), "");
}

#[test]
fn whitespace_only_input_collapses_to_empty() {
    assert_eq!(strip_for_prompt("   \n\t\n  \n"), "");
}

#[test]
fn frontmatter_is_stripped() {
    let input = "---\ntitle: Test\ntags: [a, b]\n---\nBody text\n";
    assert_eq!(strip_for_prompt(input), "Body text\n");
}

#[test]
fn only_frontmatter_yields_empty() {
    assert_eq!(strip_for_prompt("---\ntitle: Test\n---\n"), "");
}

#[test]
fn unterminated_frontmatter_is_preserved() {
    let input = "---\ntitle: Test\nBody keeps going\n";
    assert_eq!(
        strip_for_prompt(input),
        "---\ntitle: Test\nBody keeps going\n"
    );
}

#[test]
fn frontmatter_only_recognized_on_first_line() {
    let input = "intro\n---\nnot frontmatter\n---\nrest\n";
    assert_eq!(
        strip_for_prompt(input),
        "intro\n---\nnot frontmatter\n---\nrest\n"
    );
}

#[test]
fn frontmatter_with_crlf_is_stripped() {
    let input = "---\r\ntitle: Test\r\n---\r\nBody\r\n";
    assert_eq!(strip_for_prompt(input), "Body\n");
}

#[test]
fn html_comment_is_stripped() {
    assert_eq!(
        strip_for_prompt("before <!-- note --> after"),
        "before  after\n"
    );
}

#[test]
fn multiline_html_comment_is_stripped() {
    let input = "keep\n<!-- a note\nthat spans lines -->\nalso keep\n";
    assert_eq!(strip_for_prompt(input), "keep\n\nalso keep\n");
}

#[test]
fn comment_inside_backtick_fence_is_preserved() {
    let input = "```html\n<!-- keep me -->\n```\n";
    assert_eq!(strip_for_prompt(input), "```html\n<!-- keep me -->\n```\n");
}

#[test]
fn comment_inside_tilde_fence_is_preserved() {
    let input = "~~~\n<!-- keep me -->\n~~~\n";
    assert_eq!(strip_for_prompt(input), "~~~\n<!-- keep me -->\n~~~\n");
}

#[test]
fn comment_outside_fence_stripped_while_inside_kept() {
    let input = "<!-- drop -->\n```\n<!-- keep -->\n```\n<!-- drop too -->\n";
    assert_eq!(strip_for_prompt(input), "\n```\n<!-- keep -->\n```\n");
}

#[test]
fn unterminated_comment_is_preserved() {
    let input = "text <!-- never closed\nmore text\n";
    assert_eq!(
        strip_for_prompt(input),
        "text <!-- never closed\nmore text\n"
    );
}

#[test]
fn trailing_whitespace_is_trimmed_per_line() {
    assert_eq!(strip_for_prompt("a  \nb\t\nc"), "a\nb\nc\n");
}

#[test]
fn multiple_trailing_newlines_collapse_to_one() {
    assert_eq!(strip_for_prompt("body\n\n\n\n"), "body\n");
}

#[test]
fn frontmatter_and_comments_strip_together() {
    let input = "---\ndraft: true\n---\n<!-- todo: tighten -->\nThe prompt body.\n";
    assert_eq!(strip_for_prompt(input), "\nThe prompt body.\n");
}

// --- scan_placeholders ---

#[test]
fn scan_finds_simple_placeholder() {
    assert_eq!(scan_placeholders("hello {{name}}"), vec!["name"]);
}

#[test]
fn scan_returns_empty_for_no_placeholders() {
    assert!(scan_placeholders("no slots here").is_empty());
}

#[test]
fn scan_dedupes_preserving_first_occurrence_order() {
    let text = "{{b}} then {{a}} then {{b}} again {{c}} and {{a}}";
    assert_eq!(scan_placeholders(text), vec!["b", "a", "c"]);
}

#[test]
fn scan_ignores_escaped_openers() {
    assert_eq!(
        scan_placeholders(r"\{{literal}} and {{real}}"),
        vec!["real"]
    );
}

#[test]
fn scan_ignores_unbalanced_braces() {
    assert!(scan_placeholders("{{open} and {close}} and {{ }}").is_empty());
}

#[test]
fn scan_ignores_invalid_identifiers() {
    assert!(scan_placeholders("{{1abc}} {{a b}} {{a-b}} {{}}").is_empty());
}

#[test]
fn scan_accepts_underscore_and_digits_after_first() {
    assert_eq!(
        scan_placeholders("{{_private}} {{v2_final}}"),
        vec!["_private", "v2_final"]
    );
}

#[test]
fn scan_accepts_unicode_identifiers() {
    assert_eq!(scan_placeholders("{{café}} {{名前}}"), vec!["café", "名前"]);
}

#[test]
fn scan_handles_adjacent_placeholders() {
    assert_eq!(scan_placeholders("{{a}}{{b}}{{c}}"), vec!["a", "b", "c"]);
}

#[test]
fn scan_recovers_from_triple_brace() {
    assert_eq!(scan_placeholders("{{{name}}}"), vec!["name"]);
}

// --- fill_placeholders ---

fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn fill_replaces_single_occurrence() {
    let out = fill_placeholders("hello {{name}}", &values(&[("name", "world")]));
    assert_eq!(out, "hello world");
}

#[test]
fn fill_replaces_all_repeats() {
    let out = fill_placeholders("{{x}} and {{x}} and {{x}}", &values(&[("x", "y")]));
    assert_eq!(out, "y and y and y");
}

#[test]
fn fill_handles_adjacent_placeholders() {
    let out = fill_placeholders("{{a}}{{b}}", &values(&[("a", "1"), ("b", "2")]));
    assert_eq!(out, "12");
}

#[test]
fn fill_with_empty_value_removes_slot() {
    let out = fill_placeholders("a {{gap}} b", &values(&[("gap", "")]));
    assert_eq!(out, "a  b");
}

#[test]
fn fill_leaves_unknown_placeholders_intact() {
    let out = fill_placeholders("{{known}} {{unknown}}", &values(&[("known", "v")]));
    assert_eq!(out, "v {{unknown}}");
}

#[test]
fn fill_leaves_escaped_openers_untouched() {
    let out = fill_placeholders(r"\{{name}} and {{name}}", &values(&[("name", "X")]));
    assert_eq!(out, r"\{{name}} and X");
}

#[test]
fn fill_with_unicode_identifiers() {
    let out = fill_placeholders("{{名前}}さん", &values(&[("名前", "田中")]));
    assert_eq!(out, "田中さん");
}

#[test]
fn fill_with_unicode_values() {
    let out = fill_placeholders("{{greeting}}", &values(&[("greeting", "héllo wörld")]));
    assert_eq!(out, "héllo wörld");
}

#[test]
fn fill_with_empty_map_is_identity() {
    let text = "{{a}} {{b}}";
    assert_eq!(fill_placeholders(text, &HashMap::new()), text);
}

#[test]
fn fill_value_containing_placeholder_syntax_is_not_rescanned() {
    let out = fill_placeholders("{{a}}", &values(&[("a", "{{b}}"), ("b", "nope")]));
    assert_eq!(out, "{{b}}");
}
