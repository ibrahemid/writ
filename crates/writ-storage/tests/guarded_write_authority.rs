//! One guard, one terminal writer, and a test that keeps it that way.
//!
//! The guard was inlined in three places before it was a function, and the
//! copies had already drifted: one of them refused without writing the losing
//! side to disk. Five more writers are coming. What stops the fourth copy is
//! not the module boundary — `write_guarded_by_stamp` is reachable from
//! anywhere in the crate — but this test, which reads the crate's own sources
//! and says where each name is allowed to appear.

use std::path::{Path, PathBuf};

/// The module every write goes through.
const FACADE: &str = "guarded.rs";

/// The one file allowed to reach the terminal writer from outside the facade.
///
/// The notes migration (ADR-028 §4) runs once, before the watcher is started,
/// over files that are not notes yet: it has no open tab behind it, no last
/// known state to compare against and no origin to name, so there is nothing
/// for the facade to decide. It reads its own bytes back instead, which no
/// other writer does.
const EXEMPT: &str = "notes_migration.rs";

#[test]
fn the_save_guard_is_asked_in_one_place() {
    let named = files_naming("decide_save");
    assert_eq!(
        named,
        vec![FACADE.to_string()],
        "the guard belongs to the facade; a second file naming it is a second answer to \
         \"may this write land\", and the copies drift"
    );
}

#[test]
fn the_terminal_writer_has_one_caller() {
    let named = files_naming("write_guarded_by_stamp");
    assert_eq!(
        named,
        vec![FACADE.to_string(), EXEMPT.to_string()],
        "a write that reaches the terminal writer directly skips the guard, the conflict copy \
         and the origin; write through `write_note_guarded` or `create_note_guarded` instead"
    );
}

/// The non-test source files under `crates/writ-storage/src` that name
/// `identifier`, sorted, by file name.
///
/// `writ-core` owns the guard's definition and is not read: a test satisfied
/// by `writ_core::notes::guard` would pass with every storage copy still in
/// place.
fn files_naming(identifier: &str) -> Vec<String> {
    let mut named: Vec<String> = sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .into_iter()
        .filter(|path| {
            let text = std::fs::read_to_string(path).expect("read a source file");
            outside_tests(&text).contains(identifier)
        })
        .map(|path| {
            path.file_name()
                .expect("a source file has a name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    named.sort();
    named
}

/// Every `.rs` file under `dir`, at any depth.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read the crate's source folder") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            found.extend(sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    found
}

/// The file with its inline test module cut off.
///
/// A test may name whatever it is testing. The convention the crate follows is
/// one `#[cfg(test)]` module at the end of the file, so everything from the
/// first one on is test code.
fn outside_tests(text: &str) -> &str {
    match text.find("#[cfg(test)]") {
        Some(at) => &text[..at],
        None => text,
    }
}
