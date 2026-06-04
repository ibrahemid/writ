//! Fuzz target for the `writ-preview://` URL parser — the chrome↔document
//! scope boundary.
//!
//! The parser is the security gate that decides whether a request crosses
//! into the chrome scope. This target asserts two invariants over arbitrary
//! input:
//!
//! 1. **No panic.** `parse` must never panic, on any byte sequence — a
//!    panic in a release protocol handler is a denial-of-service.
//! 2. **No traversal survives.** Any `Ok(ParsedRequest)` the parser returns
//!    has a canonical path with no `..` segment and no backslash. A request
//!    that would cross the scope boundary must be `Err`, never an `Ok` whose
//!    path escapes the scope.
//!
//! Run: `cargo +nightly fuzz run preview_url_parser`
//! Seed corpus: `fuzz/corpus/preview_url_parser/`

#![no_main]

use libfuzzer_sys::fuzz_target;
use writ_core::preview::protocol::parse;

fuzz_target!(|data: &[u8]| {
    // The handler only ever receives valid UTF-8 URLs from the webview;
    // feed the parser the UTF-8 interpretation of the fuzz bytes.
    if let Ok(url) = std::str::from_utf8(data) {
        if let Ok(req) = parse(url) {
            // Invariant: a successfully-parsed path never contains a
            // traversal segment or a backslash — canonicalization rejected
            // those as Err. If one survives into an Ok, the scope boundary
            // is breakable.
            assert!(
                !req.path.split('/').any(|seg| seg == ".."),
                "traversal segment survived parse: url={url:?} path={:?}",
                req.path
            );
            assert!(
                !req.path.contains('\\'),
                "backslash survived parse: url={url:?} path={:?}",
                req.path
            );
            // The path must not begin or end with a stray separator (the
            // canonicalizer normalises these away).
            assert!(
                !req.path.starts_with('/') && !req.path.ends_with('/'),
                "non-canonical separators survived: path={:?}",
                req.path
            );
        }
    }
});
