//! The Rust half of the shared line-ending fixture.
//!
//! `src/__tests__/lib/doc-hash.fixture.test.ts` reads the same file and
//! asserts the same digests. The dirty predicate compares one side's answer
//! with the other's, so the two implementations agreeing is the whole
//! contract; the fixture's digests were produced by neither of them.

use writ_core::hash::{comparison_digest_hex, normalise_line_endings, sha256_hex};

struct Case {
    name: String,
    text: String,
    digest: String,
    raw_digest: String,
}

fn fixture() -> Vec<Case> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/line-ending-digests.json"
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
            digest: case["digest"].as_str().unwrap().to_string(),
            raw_digest: case["rawDigest"].as_str().unwrap().to_string(),
        })
        .collect()
}

#[test]
fn every_fixture_case_hashes_to_the_digest_the_editor_expects() {
    for case in fixture() {
        assert_eq!(
            comparison_digest_hex(case.text.as_bytes()),
            case.digest,
            "{}",
            case.name
        );
    }
}

#[test]
fn the_raw_digest_still_sees_a_line_ending_change() {
    // The write guard runs on the raw digest, which is what lets it notice a
    // file somebody else rewrote from CRLF to LF. Merging the two digests
    // would make that rewrite invisible and let a save land over it.
    let crlf = fixture()
        .into_iter()
        .find(|case| case.name == "crlf")
        .expect("the fixture carries a CRLF case");

    assert_eq!(sha256_hex(crlf.text.as_bytes()), crlf.raw_digest);
    assert_ne!(crlf.raw_digest, crlf.digest);
}

#[test]
fn text_with_no_carriage_return_is_not_copied() {
    let plain = b"one\ntwo\n";
    assert!(matches!(
        normalise_line_endings(plain),
        std::borrow::Cow::Borrowed(_)
    ));
}
