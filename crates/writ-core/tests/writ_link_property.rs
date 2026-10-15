//! Stable-toolchain property test for the `writ://` link grammar.
//!
//! The cargo-fuzz target (`fuzz/fuzz_targets/writ_link_parser.rs`) needs the
//! nightly toolchain, so it runs in the fuzz job. This test holds the same
//! invariants over the committed seed corpus on every merge gate: `parse`
//! never panics, an accepted path stays inside the folder, and a link built
//! for an accepted path reads back as it.

use std::fs;
use std::path::Path;

use writ_core::writ_link::{open_url, parse, WritLink};

fn assert_invariants(input: &str) {
    match parse(input) {
        Ok(WritLink::Open { path, line }) => {
            assert!(!path.is_empty(), "{input:?}");
            assert!(
                !path.starts_with('/') && !path.contains('\\') && !path.contains(':'),
                "{input:?} -> {path:?}"
            );
            assert!(
                path.split('/')
                    .all(|segment| !segment.is_empty() && segment != "." && segment != ".."),
                "{input:?} -> {path:?}"
            );
            let built = open_url(&path, line).expect("an accepted path builds a link");
            assert_eq!(
                parse(&built),
                Ok(WritLink::Open { path, line }),
                "{input:?}"
            );
        }
        Ok(WritLink::New { name: Some(name) }) => {
            assert!(!name.contains(['/', '\\', ':']), "{input:?} -> {name:?}");
        }
        Ok(WritLink::New { name: None }) | Err(_) => {}
    }
}

#[test]
fn the_committed_seed_corpus_upholds_the_invariants() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/writ_link_parser");
    let mut count = 0;
    for entry in fs::read_dir(&corpus).expect("the seed corpus") {
        let path = entry.expect("a corpus entry").path();
        let bytes = fs::read(&path).expect("a corpus file");
        if let Ok(input) = std::str::from_utf8(&bytes) {
            assert_invariants(input);
            count += 1;
        }
    }
    assert!(count >= 20, "expected the whole seed corpus, saw {count}");
}

#[test]
fn the_seeds_that_should_open_do_and_the_hostile_ones_do_not() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/writ_link_parser");
    for entry in fs::read_dir(&corpus).expect("the seed corpus") {
        let path = entry.expect("a corpus entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let input = fs::read_to_string(&path).expect("a corpus file");
        assert_eq!(
            parse(&input).is_ok(),
            name.starts_with("seed_") || name == "scheme_case",
            "{name}: {input}"
        );
    }
}
