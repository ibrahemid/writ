//! The inputs that made the scanner panic, invent links, invent tags or drop a
//! property.
//!
//! Every case here is a note a person can write. `scan` and `extract` run on
//! every save and on every file of a walk, in a build that aborts on panic, so
//! "no input panics" is part of the contract and is tested as a corpus rather
//! than as one case.

use serde_json::json;
use writ_core::notes::facts;
use writ_core::notes::links::{self, LinkKind};

/// Inputs that have broken a byte-indexing scanner, or could: escapes cut off
/// by a multi-byte character, unbalanced brackets, lone markers, and text whose
/// every byte offset is inside a character.
fn pathological() -> Vec<String> {
    let mut cases: Vec<String> = [
        "[a](%aé.md)",
        "[a](%é)",
        "[a](%)",
        "[a](%2)",
        "[a](note.md#%aé)",
        "[a](%zz.md)",
        "[a](%%%.md)",
        "[a](%C3%A9.md)",
        "[a](%c3%a9.md)",
        "[a](50%é.md)",
        "[[",
        "]]",
        "[[Note",
        "[[Note|",
        "[[Note#",
        "[[|]]",
        "[[#]]",
        "[](",
        "[](<",
        "![](",
        "[a](<é",
        "```",
        "~~~",
        "`",
        "``é`",
        "#",
        "#️⃣",
        "---\n",
        "---\né: |\n",
        "    [[é]]",
        "\t[[é]]",
        "> [[é]]",
        "- [[é]]\n\t- [[ü]]",
    ]
    .iter()
    .map(|case| (*case).to_string())
    .collect();

    // Every truncation of a string whose characters are one, two, three and
    // four bytes wide, so a slice taken at any byte offset is exercised.
    let mixed = "a[[é]] ﷽ [x](%🎉.md) #étiquette";
    for end in 0..=mixed.len() {
        cases.push(mixed.chars().take(end).collect());
        cases.push(format!(
            "[a](%{}",
            mixed.chars().take(end).collect::<String>()
        ));
    }
    cases
}

#[test]
fn no_pathological_input_panics_the_scanner() {
    for case in pathological() {
        let _ = links::scan(&case);
        let _ = facts::extract(&case);
    }
}

#[test]
fn a_percent_escape_cut_off_by_a_multibyte_character_is_left_alone() {
    let links = links::scan("see [a](%aé.md)\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Markdown);
    assert_eq!(links[0].target, "%aé");
}

#[test]
fn a_complete_percent_escape_still_decodes() {
    let links = links::scan("see [a](Caf%C3%A9%20notes.md)\n");
    assert_eq!(links[0].target, "Café notes");
}

#[test]
fn links_in_an_indented_code_block_are_not_links() {
    let scanned = links::scan("text\n\n    [[Note]]\n\n    [a](b.md)\n");
    assert!(
        scanned.is_empty(),
        "a four-space example is code, not links: {scanned:?}"
    );
    assert!(links::scan("text\n\n\t[[Note]]\n").is_empty());
}

#[test]
fn tags_and_headings_in_an_indented_code_block_do_not_count() {
    let facts = facts::extract("text\n\n    #tag\n\n    # Heading\n");
    assert!(facts.tags.is_empty(), "{:?}", facts.tags);
    assert!(facts.headings.is_empty(), "{:?}", facts.headings);
}

#[test]
fn indented_text_under_a_list_item_is_still_body() {
    let scanned = links::scan("- item\n\n    [[Note]]\n");
    assert_eq!(
        scanned
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        ["Note"],
        "a nested list line is the item's content, not a code block"
    );
    assert_eq!(links::scan("1. item\n\n    [[Note]]\n").len(), 1);
}

#[test]
fn an_indented_line_that_continues_a_paragraph_is_still_body() {
    assert_eq!(
        links::scan("a paragraph\n    [[Note]]\n").len(),
        1,
        "an indented code block cannot interrupt a paragraph"
    );
}

#[test]
fn an_indented_code_block_ends_at_the_next_unindented_line() {
    let scanned = links::scan("text\n\n    [[Code]]\n\nback [[Body]]\n");
    assert_eq!(
        scanned
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        ["Body"]
    );
}

#[test]
fn a_markdown_anchor_link_is_not_a_tag() {
    assert!(
        facts::tags("see [the plan](#the-plan) below\n").is_empty(),
        "an anchor names a heading in this note"
    );
}

#[test]
fn a_tag_in_parentheses_is_still_a_tag() {
    assert_eq!(
        facts::tags("done (#review) today\n"),
        vec![("review".to_string(), 1)]
    );
}

#[test]
fn a_literal_block_scalar_keeps_its_text() {
    let props = facts::properties("---\nnote: |\n  first line\n  second line\ntitle: A\n---\n");
    assert_eq!(
        props,
        vec![
            ("note".to_string(), json!("first line\nsecond line")),
            ("title".to_string(), json!("A")),
        ]
    );
}

#[test]
fn a_folded_block_scalar_joins_its_lines() {
    let props = facts::properties("---\nsummary: >\n  one\n  two\n\n  three\n---\n");
    assert_eq!(
        props,
        vec![("summary".to_string(), json!("one two\nthree"))]
    );
}

#[test]
fn a_block_scalar_with_a_chomping_indicator_reads_the_same() {
    let props = facts::properties("---\nnote: |-\n  kept\n---\n");
    assert_eq!(props, vec![("note".to_string(), json!("kept"))]);
}

#[test]
fn a_block_scalar_ends_at_the_next_key() {
    let props = facts::properties("---\nnote: |\n  body\n\ntags: [a]\n---\n");
    assert_eq!(
        props,
        vec![
            ("note".to_string(), json!("body")),
            ("tags".to_string(), json!(["a"])),
        ]
    );
}
