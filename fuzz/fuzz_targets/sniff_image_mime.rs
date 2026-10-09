//! Fuzz target for `sniff_image_mime`, which decides from a file's leading
//! bytes whether the preview serves it as an image.
//!
//! The bytes are whatever file a Markdown image reference names, so this
//! target asserts over arbitrary input:
//!
//! 1. **No panic.** The sniff runs when a file opens in the Inline layout and
//!    on the preview's asset route; a panic aborts the app.
//! 2. **Only a served type comes back.** Any `Some` is one of the image MIME
//!    types the preview serves.
//!
//! Run: `cargo +nightly fuzz run sniff_image_mime`
//! Seed corpus: `fuzz/corpus/sniff_image_mime/`

#![no_main]

use libfuzzer_sys::fuzz_target;
use writ_core::preview::protocol::sniff_image_mime;

const SERVED: [&str; 7] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/avif",
    "image/svg+xml",
];

fuzz_target!(|data: &[u8]| {
    if let Some(mime) = sniff_image_mime(data) {
        assert!(
            SERVED.contains(&mime),
            "unexpected type {mime:?} for {data:?}"
        );
    }
});
