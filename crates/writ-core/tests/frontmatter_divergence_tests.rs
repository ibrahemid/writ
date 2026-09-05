//! Three splitters, one rule.
//!
//! `notes::links::split_frontmatter` reads the block so the index can store a
//! note's properties, `prompt::strip::strip_frontmatter` drops it from a
//! prompt, and `writ_render::split_frontmatter` hides it from the preview and
//! from the site. They cannot be one function — writ-render compiles to wasm
//! and depends on nothing else in the workspace — so this runs all three over
//! one table and fails the moment any of them moves.
//!
//! The table extends the six cases
//! `crates/writ-render/tests/frontmatter_tests.rs` carries with the inputs a
//! block-reading splitter can get wrong: a fence that is nearly a delimiter, a
//! delimiter with trailing whitespace, an empty block, and a body that opens
//! with one.

use writ_core::notes::links::split_frontmatter;
use writ_core::prompt::strip::strip_frontmatter;

/// Inputs all three splitters must agree on, byte for byte.
const CASES: &[&str] = &[
    "---\ntitle: Test\ntags: [a, b]\n---\nBody text\n",
    "---\ntitle: Test\n---\n",
    "---\ntitle: Test\nBody keeps going\n",
    "intro\n---\nnot frontmatter\n---\nrest\n",
    "---\r\ntitle: Test\r\n---\r\nBody\r\n",
    "Body only\n",
    "",
    "---\n",
    "---\n---\n",
    "---\n---\nbody\n",
    "---   \ntitle: Test\n---   \nBody\n",
    "----\ntitle: Test\n----\nBody\n",
    "--- \n---\n",
    " ---\ntitle: Test\n---\nBody\n",
    "---\ntitle: Test\n---",
    "---\n```\n---\n```\n---\nBody\n",
    "---\ntitle: \"---\"\n---\nBody\n",
    "---\nملاحظات: نعم\n---\nنص\n",
];

#[test]
fn every_splitter_leaves_the_same_body() {
    for input in CASES {
        let (_, core_body) = split_frontmatter(input);
        assert_eq!(
            core_body,
            strip_frontmatter(input),
            "notes and prompt disagree on {input:?}"
        );
        assert_eq!(
            core_body,
            writ_render::split_frontmatter(input).body,
            "notes and render disagree on {input:?}"
        );
    }
}

#[test]
fn every_splitter_recognises_the_same_block() {
    for input in CASES {
        let (core_raw, _) = split_frontmatter(input);
        assert_eq!(
            core_raw,
            writ_render::split_frontmatter(input).raw,
            "notes and render disagree on the block in {input:?}"
        );
    }
}

#[test]
fn the_block_and_the_body_rebuild_the_input() {
    for input in CASES {
        let (raw, body) = split_frontmatter(input);
        assert_eq!(
            format!("{}{body}", raw.unwrap_or_default()),
            *input,
            "the split lost bytes of {input:?}"
        );
    }
}
