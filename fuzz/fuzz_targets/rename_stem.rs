//! Fuzz target for the name a typed rename earns: `rename_stem` and the
//! `sanitize_title` it ends in.
//!
//! Both run on whatever a person types into a tab, the sidebar, `writ rename`
//! or an MCP client, so this target asserts over arbitrary text:
//!
//! 1. **No panic.** A panic here aborts the app, the CLI or the MCP server
//!    (`panic = "abort"` in release builds).
//! 2. **A name is one non-empty path component.** Any `Some` either function
//!    returns is non-empty, carries no `/` or `\`, and fits in
//!    `MAX_TITLE_BYTES`, because the caller joins it onto the note's folder.
//!
//! The input is `<note file name>/<typed name>`, split at the first `/`, so
//! the note's own extension is fuzzed as well as what is typed. An input with
//! no `/` is typed against `note.md`.
//!
//! Run: `cargo +nightly fuzz run rename_stem`
//! Seed corpus: `fuzz/corpus/rename_stem/`

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use writ_core::notes::{rename_stem, sanitize_title, MAX_TITLE_BYTES};

fn assert_one_component(stem: &str, input: &str) {
    assert!(!stem.is_empty(), "empty name survived: input={input:?}");
    assert!(
        !stem.contains('/') && !stem.contains('\\'),
        "separator survived: input={input:?} stem={stem:?}"
    );
    assert!(
        stem.len() <= MAX_TITLE_BYTES,
        "name over the byte limit: input={input:?} stem={stem:?}"
    );
}

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    let (note_name, typed) = input.split_once('/').unwrap_or(("note.md", input));
    let note = Path::new("/notes").join(note_name);

    if let Some(stem) = rename_stem(&note, typed) {
        assert_one_component(&stem, input);
    }
    if let Some(title) = sanitize_title(typed) {
        assert_one_component(&title, input);
    }
});
