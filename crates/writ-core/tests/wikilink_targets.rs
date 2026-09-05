//! The Rust half of the shared wikilink fixture.
//!
//! `src/__tests__/lib/wikilink.fixture.test.ts` reads the same file and runs
//! the editor's scanner over the same text. Two implementations answer "what
//! note does this name": this one decides what the index stores and what a
//! link resolves to, the editor's decides what is painted and what a click
//! follows. A link one finds and the other does not is a link that goes
//! nowhere, so the fixture is what keeps them from drifting apart.
//!
//! The name comes from [`parse_wikilink`], which is what resolution reads.
//! `RawLink::wikilink_target` strips a note extension a second time, so it
//! answers `Note` where resolution answers `Note.md`; that is a separate
//! question from this one.

use writ_core::notes::links::{parse_wikilink, scan, LinkKind};

/// The raw text inside each `[[…]]` the scanner finds in `text`.
fn targets(text: &str) -> Vec<String> {
    scan(text)
        .into_iter()
        .filter(|link| link.kind == LinkKind::Wikilink)
        .map(|link| {
            text[link.byte_range.clone()]
                .trim_start_matches("[[")
                .trim_end_matches("]]")
                .to_string()
        })
        .collect()
}

fn strings(value: &serde_json::Value, field: &str) -> Vec<String> {
    value[field]
        .as_array()
        .unwrap_or_else(|| panic!("{field} is an array"))
        .iter()
        .map(|one| one.as_str().expect("a string").to_string())
        .collect()
}

#[test]
fn the_scanner_finds_what_the_fixture_says() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wikilink-targets.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture is readable");
    let raw: serde_json::Value = serde_json::from_str(&text).expect("fixture is json");
    let cases = raw["cases"].as_array().expect("cases is an array");
    assert!(cases.len() > 20, "the fixture carries a corpus");

    for case in cases {
        let name = case["name"].as_str().expect("a name");
        let body = case["text"].as_str().expect("a text");
        let found = targets(body);
        assert_eq!(found, strings(case, "targets"), "targets in case {name}");
        let names: Vec<String> = found.iter().map(|one| parse_wikilink(one).name).collect();
        assert_eq!(names, strings(case, "names"), "names in case {name}");
    }
}

#[test]
fn every_deliberate_difference_says_what_it_is_for() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wikilink-targets.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture is readable");
    let raw: serde_json::Value = serde_json::from_str(&text).expect("fixture is json");
    for case in raw["cases"].as_array().expect("cases is an array") {
        if case.get("editorTargets").is_none() {
            continue;
        }
        let name = case["name"].as_str().expect("a name");
        assert!(
            case["why"].as_str().is_some_and(|w| !w.is_empty()),
            "case {name} differs on purpose and has to say what for"
        );
    }
}
