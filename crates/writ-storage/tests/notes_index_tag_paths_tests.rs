//! The notes one tag names: `NotesIndex::paths_for_tag` (ADR-036).
//!
//! The tag list says how many notes carry a tag; this says which. What is
//! tested here is what the sidebar's tag filter stands on: a whole-tag match,
//! a nested tag standing on its own rather than under its first segment, and
//! a note counted once however often it writes the tag.

use std::path::Path;

use tempfile::TempDir;
use writ_storage::notes_index::{self, NotesIndexStore};

fn write_note(notes: &Path, name: &str, body: &str) {
    let path = notes.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, body).expect("write note");
}

/// An index over a folder of notes, walked once.
fn indexed(notes: &[(&str, &str)]) -> (TempDir, std::path::PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open_database");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);

    let root = dir.path().join("notes");
    std::fs::create_dir_all(&root).expect("create notes dir");
    for (name, body) in notes {
        write_note(&root, name, body);
    }

    let index = NotesIndexStore::open(&db_path).expect("index");
    index
        .reconcile(&root, &|| false, &|_| false)
        .expect("reconcile");
    (dir, root, index)
}

fn key(root: &Path, name: &str) -> String {
    notes_index::index_key(&root.join(name))
}

#[test]
fn a_tag_names_the_notes_carrying_it() {
    let (_dir, root, index) = indexed(&[
        ("One.md", "#idea\n"),
        ("Two.md", "#idea and #draft\n"),
        ("Three.md", "#draft\n"),
    ]);

    assert_eq!(
        index.paths_for_tag("idea").expect("paths"),
        vec![key(&root, "One.md"), key(&root, "Two.md")]
    );
}

#[test]
fn a_nested_tag_is_not_a_note_of_its_first_segment() {
    let (_dir, root, index) =
        indexed(&[("Alpha.md", "#project/alpha\n"), ("Plan.md", "#project\n")]);

    assert_eq!(
        index.paths_for_tag("project").expect("paths"),
        vec![key(&root, "Plan.md")]
    );
    assert_eq!(
        index.paths_for_tag("project/alpha").expect("paths"),
        vec![key(&root, "Alpha.md")]
    );
}

#[test]
fn a_note_tagged_twice_is_named_once() {
    let (_dir, root, index) = indexed(&[("One.md", "#idea at the top\n\nand #idea again\n")]);

    assert_eq!(
        index.paths_for_tag("idea").expect("paths"),
        vec![key(&root, "One.md")]
    );
}

#[test]
fn a_tag_nothing_carries_names_no_notes() {
    let (_dir, _root, index) = indexed(&[("One.md", "#idea\n")]);

    assert!(index.paths_for_tag("nothing").expect("paths").is_empty());
}
