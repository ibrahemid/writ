//! The Rust half of the shared token-estimate fixture.
//!
//! `src/__tests__/lib/token-estimate.fixture.test.ts` reads the same file: the
//! status bar estimates tokens from the document's counts instead of sending
//! the text here, so the two have to give the same number for the same text.

use writ_core::prompt::estimate_tokens;

struct Case {
    name: String,
    text: String,
    tokens: usize,
}

fn fixture() -> Vec<Case> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/token-estimates.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture is readable");
    let raw: serde_json::Value = serde_json::from_str(&text).expect("fixture is json");
    raw["cases"]
        .as_array()
        .expect("cases is an array")
        .iter()
        .map(|case| Case {
            name: case["name"].as_str().unwrap().to_string(),
            text: case["text"].as_str().unwrap().to_string(),
            tokens: usize::try_from(case["tokens"].as_u64().unwrap()).unwrap(),
        })
        .collect()
}

#[test]
fn every_fixture_case_estimates_to_the_tokens_the_status_bar_shows() {
    let cases = fixture();
    assert!(cases.len() >= 5, "the fixture carries its cases");
    for case in cases {
        assert_eq!(estimate_tokens(&case.text), case.tokens, "{}", case.name);
    }
}
