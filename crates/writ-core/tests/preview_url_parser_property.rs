//! Stable-toolchain property test for the `writ-preview://` URL parser.
//!
//! The cargo-fuzz target (`fuzz/fuzz_targets/preview_url_parser.rs`) needs
//! the nightly toolchain + libfuzzer, so it runs in the fuzz job, not the
//! merge gates. This test enforces the *same* invariants on stable over two
//! input banks — the committed seed corpus and a generated adversarial set —
//! so the parser's scope-boundary guarantee is a green merge gate, not only
//! a nightly job:
//!
//! 1. `parse` never panics.
//! 2. Any `Ok` path is canonical: no `..` segment, no backslash, no leading
//!    or trailing separator — i.e. nothing that would cross the chrome↔
//!    document boundary ever survives into an accepted request.

use std::fs;
use std::path::Path;

use writ_core::preview::protocol::{parse, ParsedRequest};

fn assert_invariants(url: &str) {
    // Invariant 1: no panic. (Reaching the next line is the assertion.)
    let result = parse(url);

    // Invariant 2: an accepted request is canonical.
    if let Ok(ParsedRequest { path, .. }) = result {
        assert!(
            !path.split('/').any(|seg| seg == ".."),
            "traversal `..` survived parse: url={url:?} path={path:?}",
        );
        assert!(
            !path.contains('\\'),
            "backslash survived parse: url={url:?} path={path:?}",
        );
        assert!(
            !path.starts_with('/') && !path.ends_with('/'),
            "non-canonical separator survived: url={url:?} path={path:?}",
        );
    }
}

#[test]
fn committed_seed_corpus_upholds_the_invariants() {
    // The fuzz seed corpus is the documented malicious-fixture set; run the
    // stable property test over the same bytes so the two stay in lockstep.
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/preview_url_parser");
    assert!(
        corpus.is_dir(),
        "fuzz corpus dir missing: {}",
        corpus.display()
    );

    let mut count = 0;
    for entry in fs::read_dir(&corpus).expect("read corpus dir") {
        let path = entry.expect("corpus entry").path();
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path).expect("read corpus file");
        // Corpus files are UTF-8 URL strings.
        if let Ok(url) = std::str::from_utf8(&bytes) {
            assert_invariants(url);
            count += 1;
        }
    }
    assert!(
        count >= 20,
        "expected the full seed corpus, saw {count} files"
    );
}

#[test]
fn generated_adversarial_inputs_uphold_the_invariants() {
    // Structured fuzzing on stable: cross-product of schemes, scopes, and
    // hostile path fragments, plus raw control/encoding noise.
    let schemes = [
        "writ-preview",
        "WRIT-PREVIEW",
        "https",
        "writ-workspace",
        "",
        "x",
    ];
    let scopes = ["chrome", "document", "attacker", "", "Chrome"];
    let fragments = [
        "",
        "/",
        "//",
        "/x",
        "/../x",
        "/..%2f",
        "/%2e%2e/",
        "/%2E%2E/",
        "/%252e%252e/",
        "/..\\x",
        "/foo%00bar",
        "/foo%2",
        "/foo%xx",
        "/C:%2Fwindows",
        "/a/b/c",
        "/./././",
        "/...",
        "/a%2fb",
        "/\u{202e}x",
        "/üñîçødé",
        "/x?q=1#h",
        "/x?../../y",
    ];

    for scheme in schemes {
        for scope in scopes {
            for frag in fragments {
                assert_invariants(&format!("{scheme}://{scope}{frag}"));
            }
        }
    }

    // Raw byte-ish noise rendered as strings the webview could conceivably
    // hand us.
    for raw in [
        "\u{0}",
        "%",
        "%%",
        "%0",
        "%g0",
        "://",
        "writ-preview:",
        "writ-preview:/",
        "writ-preview://",
        "writ-preview://chrome",
        ":////",
        "writ-preview://document/".repeat(1000).as_str(),
    ] {
        assert_invariants(raw);
    }
}
