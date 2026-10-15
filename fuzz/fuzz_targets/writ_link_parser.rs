//! Fuzz target for the `writ://` link grammar (ADR-052).
//!
//! Any web page or program on the machine can hand Writ a `writ://` link, so
//! this target asserts over arbitrary text:
//!
//! 1. **No panic.** `parse` and `open_url` must never panic.
//! 2. **No accepted path leaves the folder.** Any `Open` path is non-empty,
//!    relative, and has no backslash, colon, control character, or empty,
//!    `.` or `..` segment. Any `New` name holds no separator or colon.
//! 3. **A link Writ builds reads back.** `open_url` on an accepted path
//!    gives a link that parses to the same path and line.
//!
//! Run: `cargo +nightly fuzz run writ_link_parser`
//! Seed corpus: `fuzz/corpus/writ_link_parser/`

#![no_main]

use libfuzzer_sys::fuzz_target;
use writ_core::writ_link::{open_url, parse, WritLink};

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    match parse(input) {
        Ok(WritLink::Open { path, line }) => {
            assert!(!path.is_empty(), "empty path: input={input:?}");
            assert!(
                !path.starts_with('/') && !path.contains('\\') && !path.contains(':'),
                "path not relative: input={input:?} path={path:?}"
            );
            assert!(
                !path.chars().any(char::is_control),
                "control character survived: input={input:?}"
            );
            assert!(
                path.split('/')
                    .all(|segment| !segment.is_empty() && segment != "." && segment != ".."),
                "segment out of the folder: input={input:?} path={path:?}"
            );
            assert_ne!(line, Some(0), "line 0 accepted: input={input:?}");
            let built = open_url(&path, line).expect("an accepted path builds a link");
            assert_eq!(
                parse(&built),
                Ok(WritLink::Open {
                    path: path.clone(),
                    line
                }),
                "round trip: input={input:?} built={built:?}"
            );
        }
        Ok(WritLink::New { name: Some(name) }) => {
            assert!(
                !name.contains(['/', '\\', ':']) && !name.trim().is_empty(),
                "name not a file name: input={input:?} name={name:?}"
            );
        }
        Ok(WritLink::New { name: None }) | Err(_) => {}
    }
    let _ = open_url(input, None);
});
