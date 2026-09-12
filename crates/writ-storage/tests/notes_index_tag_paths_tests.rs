//! The notes one tag names: `NotesIndex::paths_for_tag` (ADR-036).
//!
//! The tag list says how many notes carry a tag; this says which. What is
//! tested here is what the sidebar's tag filter stands on: a tag names its
//! own notes and the notes under it, so a parent row filters its family and
//! a child row filters its own subtree; a sibling that merely shares a prefix
//! is outside the family; and a note is counted once however often it writes
//! the tag.

use std::path::Path;

use tempfile::TempDir;
use writ_storage::notes_index::{self, NotesIndexStore};
use writ_storage::workspace_store;

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
fn a_tag_names_its_own_notes_and_the_notes_under_it() {
    let (_dir, root, index) =
        indexed(&[("Alpha.md", "#project/alpha\n"), ("Plan.md", "#project\n")]);

    assert_eq!(
        index.paths_for_tag("project").expect("paths"),
        vec![key(&root, "Alpha.md"), key(&root, "Plan.md")]
    );
    assert_eq!(
        index.paths_for_tag("project/alpha").expect("paths"),
        vec![key(&root, "Alpha.md")]
    );
}

#[test]
fn a_tag_three_levels_down_is_under_both_of_its_parents() {
    let (_dir, root, index) = indexed(&[
        ("Deep.md", "#project/alpha/x\n"),
        ("Alpha.md", "#project/alpha\n"),
        ("Plan.md", "#project\n"),
    ]);

    assert_eq!(
        index.paths_for_tag("project").expect("paths"),
        vec![
            key(&root, "Alpha.md"),
            key(&root, "Deep.md"),
            key(&root, "Plan.md"),
        ]
    );
    assert_eq!(
        index.paths_for_tag("project/alpha").expect("paths"),
        vec![key(&root, "Alpha.md"), key(&root, "Deep.md")]
    );
    assert_eq!(
        index.paths_for_tag("project/alpha/x").expect("paths"),
        vec![key(&root, "Deep.md")]
    );
}

#[test]
fn a_sibling_sharing_a_prefix_is_not_under_the_tag() {
    let (_dir, root, index) = indexed(&[
        ("Plan.md", "#project\n"),
        ("Many.md", "#projects\n"),
        ("Dash.md", "#project-x\n"),
        ("Under.md", "#projects/one\n"),
    ]);

    assert_eq!(
        index.paths_for_tag("project").expect("paths"),
        vec![key(&root, "Plan.md")]
    );
    assert_eq!(
        index.paths_for_tag("projects").expect("paths"),
        vec![key(&root, "Many.md"), key(&root, "Under.md")]
    );
}

#[test]
fn an_underscore_in_the_asked_tag_is_a_character_and_not_a_wildcard() {
    let (_dir, root, index) = indexed(&[("Mine.md", "#my_tag/one\n"), ("Other.md", "#myxtag/y\n")]);

    assert_eq!(
        index.paths_for_tag("my_tag").expect("paths"),
        vec![key(&root, "Mine.md")]
    );
}

#[test]
fn a_parent_no_note_carries_still_names_the_notes_under_it() {
    let (_dir, root, index) = indexed(&[
        ("Alpha.md", "#project/alpha\n"),
        ("Beta.md", "#project/beta\n"),
    ]);

    assert_eq!(
        index.paths_for_tag("project").expect("paths"),
        vec![key(&root, "Alpha.md"), key(&root, "Beta.md")]
    );
}

#[test]
fn the_casing_a_tag_is_asked_with_still_finds_the_family() {
    let (_dir, root, index) =
        indexed(&[("Alpha.md", "#Project/Alpha\n"), ("Plan.md", "#project\n")]);

    assert_eq!(
        index.paths_for_tag("Project").expect("paths"),
        vec![key(&root, "Alpha.md"), key(&root, "Plan.md")]
    );
    assert_eq!(
        index.paths_for_tag("PROJECT/alpha").expect("paths"),
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

#[test]
fn the_paths_a_tag_names_are_the_paths_the_file_tree_lists() {
    // The sidebar filters the tree by comparing these two strings. Both sides
    // canonicalise, so a note reached through the folder listing and the same
    // note reached through the index spell their path the same way; were they
    // to drift, the filter would empty the tree instead of narrowing it.
    let (_dir, root, index) = indexed(&[("Drafts/Launch.md", "#idea\n")]);

    let listing = workspace_store::list_dir(&root, &root.join("Drafts")).expect("list_dir");
    let listed = listing
        .iter()
        .find(|entry| entry.name == "Launch.md")
        .expect("the note is in the listing");

    assert_eq!(
        index.paths_for_tag("idea").expect("paths"),
        vec![listed.path.clone()]
    );
    assert_eq!(listed.path, key(&root, "Drafts/Launch.md"));
}
