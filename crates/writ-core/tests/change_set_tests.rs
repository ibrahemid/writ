//! The Rust half of the shared change-set fixture, and the change sets Rust
//! refuses.
//!
//! `src/__tests__/editor/change-set.fixture.test.ts` reads the same file: the
//! editor's large tiers save by sending a CodeMirror change set, so the text
//! Rust writes has to be the document the editor holds.

use serde_json::{json, Value};
use writ_core::buffer::change_set::{ChangeSection, ChangeSet, ChangeSetError};

struct Case {
    name: String,
    base: String,
    changes: Value,
    result: String,
}

fn fixture() -> Vec<Case> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/change-sets.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture is readable");
    let raw: Value = serde_json::from_str(&text).expect("fixture is json");
    raw["cases"]
        .as_array()
        .expect("cases is an array")
        .iter()
        .map(|case| Case {
            name: case["name"].as_str().unwrap().to_string(),
            base: case["base"].as_str().unwrap().to_string(),
            changes: case["changes"].clone(),
            result: case["result"].as_str().unwrap().to_string(),
        })
        .collect()
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[test]
fn every_fixture_change_set_gives_the_document_codemirror_gives() {
    let cases = fixture();
    assert!(cases.len() >= 10, "the fixture carries its cases");
    for case in cases {
        let changes = ChangeSet::from_json(&case.changes).expect(&case.name);
        assert_eq!(
            changes.base_length(),
            utf16_len(&case.base),
            "{}",
            case.name
        );
        assert_eq!(
            changes.result_length(),
            utf16_len(&case.result),
            "{}",
            case.name
        );
        assert_eq!(
            changes.apply(&case.base).as_deref(),
            Ok(case.result.as_str()),
            "{}",
            case.name
        );
    }
}

#[test]
fn a_change_set_reads_through_serde_as_it_does_from_a_value() {
    let changes: ChangeSet = serde_json::from_value(json!([3, [2, "a", "b"], [1]])).unwrap();
    assert_eq!(
        changes.sections(),
        &[
            ChangeSection::Keep(3),
            ChangeSection::Replace {
                deleted: 2,
                inserted: "a\nb".to_string()
            },
            ChangeSection::Replace {
                deleted: 1,
                inserted: String::new()
            },
        ]
    );
}

#[test]
fn a_change_set_made_against_a_longer_text_is_refused() {
    let changes = ChangeSet::from_json(&json!([10, [0, "x"]])).unwrap();
    assert_eq!(
        changes.apply("short"),
        Err(ChangeSetError::LengthMismatch {
            expected: 10,
            actual: 5
        })
    );
}

#[test]
fn a_change_set_made_against_a_shorter_text_is_refused() {
    let changes = ChangeSet::from_json(&json!([2, [1, "x"]])).unwrap();
    assert_eq!(
        changes.apply("longer"),
        Err(ChangeSetError::LengthMismatch {
            expected: 3,
            actual: 6
        })
    );
}

#[test]
fn a_section_that_ends_inside_a_surrogate_pair_is_refused() {
    let changes = ChangeSet::from_json(&json!([2, [1, "x"], 1])).unwrap();
    assert_eq!(
        changes.apply("a😀b"),
        Err(ChangeSetError::SplitsCharacter(2))
    );
}

#[test]
fn values_outside_the_form_are_refused() {
    for value in [
        json!({ "keep": 1 }),
        json!([-1]),
        json!([1.5]),
        json!([[]]),
        json!([[1, 2]]),
        json!(["text"]),
    ] {
        assert!(
            matches!(
                ChangeSet::from_json(&value),
                Err(ChangeSetError::Malformed(_))
            ),
            "{value}"
        );
    }
}
