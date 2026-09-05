//! IPC coverage for the notes-index reads: `resolve_note_link`, `note_facts`
//! and `note_name_candidates` (ADR-034).
//!
//! Each command is exercised through its Tauri-free inner function against a
//! real index over a real folder, so the assertions cover the path spelling and
//! the DTO shape the editor receives rather than only the policy underneath.

use std::path::Path;

use tempfile::TempDir;
use writ_storage::notes_index::{self, NotesIndexStore};
use writ_tauri_lib::commands::note_index::{
    note_facts_inner, note_name_candidates_inner, resolve_note_link_inner,
};

const LIB_RS: &str = include_str!("../src/lib.rs");

fn write_note(notes: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = notes.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, body).expect("write note");
    path
}

/// An index over a folder of notes, walked once.
fn indexed(notes: &[(&str, &str)]) -> (TempDir, std::path::PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
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

#[test]
fn resolve_note_link_reports_the_note_a_target_names() {
    let (_dir, root, index) = indexed(&[
        ("source.md", "see [[Target]]\n"),
        ("Target.md", "# Target\n\n## Later Part\n"),
    ]);

    let resolution = resolve_note_link_inner(
        &index,
        root.join("source.md").to_str().expect("utf-8 path"),
        "Target",
    )
    .expect("resolve");
    assert_eq!(resolution.status, "resolved");
    assert_eq!(
        resolution.path.as_deref(),
        Some(notes_index::index_key(&root.join("Target.md")).as_str())
    );
    assert!(resolution.candidates.is_empty());
    assert_eq!(resolution.heading_line, None);
}

#[test]
fn resolve_note_link_reports_the_line_of_a_heading_the_target_names() {
    let (_dir, root, index) = indexed(&[
        ("source.md", "see [[Target#Later Part]]\n"),
        ("Target.md", "# Target\n\n## Later Part\n"),
    ]);

    let resolution = resolve_note_link_inner(
        &index,
        root.join("source.md").to_str().expect("utf-8 path"),
        "Target#Later Part",
    )
    .expect("resolve");
    assert_eq!(resolution.status, "resolved");
    assert_eq!(resolution.heading_line, Some(3));
}

#[test]
fn resolve_note_link_hands_back_every_candidate_rather_than_guessing() {
    let (_dir, root, index) = indexed(&[
        ("from/source.md", "see [[Note]]\n"),
        ("a/Note.md", "one\n"),
        ("b/Note.md", "two\n"),
    ]);

    let resolution = resolve_note_link_inner(
        &index,
        root.join("from/source.md").to_str().expect("utf-8 path"),
        "Note",
    )
    .expect("resolve");
    assert_eq!(resolution.status, "ambiguous");
    assert_eq!(resolution.path, None);
    assert_eq!(resolution.candidates.len(), 2);
}

#[test]
fn resolve_note_link_reports_a_target_with_no_note_behind_it() {
    let (_dir, root, index) = indexed(&[("source.md", "see [[Nothing]]\n")]);

    let resolution = resolve_note_link_inner(
        &index,
        root.join("source.md").to_str().expect("utf-8 path"),
        "Nothing",
    )
    .expect("resolve");
    assert_eq!(resolution.status, "missing");
    assert_eq!(resolution.path, None);
    assert!(resolution.candidates.is_empty());
}

#[test]
fn note_facts_returns_all_four_kinds_of_fact() {
    let (_dir, root, index) = indexed(&[
        (
            "note.md",
            "---\ntitle: Weekly\n---\n# Heading\n\n#inbox and [[Other]]\n",
        ),
        ("Other.md", "the other one\n"),
    ]);

    let facts = note_facts_inner(&index, root.join("note.md").to_str().expect("utf-8 path"))
        .expect("note facts");
    assert_eq!(facts.properties.len(), 1);
    assert_eq!(facts.properties[0].key, "title");
    assert_eq!(facts.properties[0].value_json, "\"Weekly\"");
    assert_eq!(facts.tags.len(), 1);
    assert_eq!(facts.tags[0].tag, "inbox");
    assert_eq!(facts.headings.len(), 1);
    assert_eq!(facts.headings[0].slug, "heading");
    assert_eq!(facts.links.len(), 1);
    assert_eq!(facts.links[0].kind, "wikilink");
    assert!(facts.links[0].to_path.is_some());
}

#[test]
fn note_facts_on_a_note_the_index_does_not_hold_is_empty_rather_than_an_error() {
    let (_dir, root, index) = indexed(&[("note.md", "body\n")]);
    let facts = note_facts_inner(
        &index,
        root.join("never-indexed.md").to_str().expect("utf-8 path"),
    )
    .expect("note facts");
    assert_eq!(facts, Default::default());
}

#[test]
fn note_name_candidates_ranks_the_note_names_and_honours_the_limit() {
    let (_dir, root, index) = indexed(&[
        ("Weekly review.md", "one\n"),
        ("Weekly plan.md", "two\n"),
        ("Groceries.md", "three\n"),
    ]);

    let hits = note_name_candidates_inner(&index, "weekly", &root, None).expect("candidates");
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| hit.name.starts_with("Weekly")));

    let one = note_name_candidates_inner(&index, "weekly", &root, Some(1)).expect("candidates");
    assert_eq!(one.len(), 1);
}

#[test]
fn note_name_candidates_rank_on_the_path_inside_the_notes_folder() {
    let (_dir, root, index) = indexed(&[("alpha.md", "one\n"), ("projects/beta.md", "two\n")]);

    // A folder inside the notes folder is part of what a candidate is matched
    // against, which is only true while the completion hands `search_names` the
    // notes root it hands quick open. Without it a candidate falls back to its
    // bare filename and this query finds nothing.
    let hits = note_name_candidates_inner(&index, "projbeta", &root, None).expect("candidates");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "beta.md");
    assert_eq!(
        hits[0].path,
        notes_index::index_key(&root.join("projects").join("beta.md"))
    );
}

#[test]
fn note_name_candidates_answers_an_empty_query_with_nothing() {
    let (_dir, root, index) = indexed(&[("Note.md", "body\n")]);
    assert!(note_name_candidates_inner(&index, "   ", &root, None)
        .expect("candidates")
        .is_empty());
}

#[test]
fn all_three_note_index_commands_are_registered() {
    for command in [
        "commands::note_index::resolve_note_link",
        "commands::note_index::note_facts",
        "commands::note_index::note_name_candidates",
    ] {
        assert!(
            LIB_RS.contains(command),
            "{command} is not in the invoke handler, so the editor cannot call it"
        );
    }
}
