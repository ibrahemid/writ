//! Fuzz target for the `writ-preview://` URL parser — the chrome↔document
//! scope boundary.
//!
//! The parser is the security gate that decides whether a request crosses
//! into the chrome scope. This target asserts three invariants over
//! arbitrary input:
//!
//! 1. **No panic.** `parse` and `split_asset_request` must never panic, on
//!    any byte sequence — a panic in a release protocol handler is a
//!    denial-of-service.
//! 2. **No traversal survives.** Any `Ok(ParsedRequest)` the parser returns
//!    has a canonical path with no `..` segment and no backslash. A request
//!    that would cross the scope boundary must be `Err`, never an `Ok` whose
//!    path escapes the scope.
//! 3. **The note-asset sub-path splits cleanly.** `split_asset_request` is
//!    the other pure gate on the route ADR-035 adds under the document
//!    scope; the relative path it hands the resolver carries no traversal
//!    segment and no leading separator. Only the pure split is fuzzed — the
//!    containment check needs a real notes folder.
//!
//! Run: `cargo +nightly fuzz run preview_url_parser`
//! Seed corpus: `fuzz/corpus/preview_url_parser/`

#![no_main]

use libfuzzer_sys::fuzz_target;
use writ_core::preview::protocol::{parse, split_asset_request, PreviewScope};

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
            if req.scope == PreviewScope::Document {
                if let Some(asset) = split_asset_request(&req.path) {
                    assert!(
                        !asset.relative.split('/').any(|seg| seg == ".." || seg == "."),
                        "traversal segment survived the asset split: url={url:?} relative={:?}",
                        asset.relative
                    );
                    assert!(
                        !asset.relative.starts_with('/')
                            && !asset.buffer_id.is_empty()
                            && !asset.token.is_empty(),
                        "malformed asset request accepted: url={url:?}",
                    );
                }
            }
        }
    }
});
