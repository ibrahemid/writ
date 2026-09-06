//! The whole folder as nodes and edges: `NotesIndex::graph` (ADR-036).
//!
//! The link rules themselves are tested in `writ_core::notes::links` and the
//! rows they produce in `notes_index_facts_tests`. What is tested here is the
//! shape the graph surfaces read: which files are nodes, which links become
//! edges, and what a repeated link, a self-link and an ambiguous target do to
//! the answer.

use std::path::Path;

use tempfile::TempDir;
use writ_storage::notes_index::{self, GraphRows, NotesIndexStore};

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

fn edge_count(rows: &GraphRows, from: &str, to: &str) -> Option<usize> {
    rows.edges
        .iter()
        .find(|edge| edge.from_path == from && edge.to_path == to)
        .map(|edge| edge.count)
}

fn node_paths(rows: &GraphRows) -> Vec<&str> {
    rows.nodes.iter().map(|node| node.path.as_str()).collect()
}

#[test]
fn a_resolved_link_is_an_edge_and_an_unresolved_one_is_not() {
    let (_dir, root, index) = indexed(&[
        ("source.md", "see [[Target]] and [[Nowhere]]\n"),
        ("Target.md", "# Target\n"),
    ]);

    let rows = index.graph(&root).expect("graph");

    let source = key(&root, "source.md");
    let target = key(&root, "Target.md");
    assert_eq!(edge_count(&rows, &source, &target), Some(1));
    assert_eq!(
        rows.edges.len(),
        1,
        "a link naming no note is not an edge: {:?}",
        rows.edges
    );
    assert!(
        node_paths(&rows).contains(&source.as_str()),
        "the note writing the unresolved link is still a node"
    );
}

#[test]
fn an_ambiguous_target_draws_no_edge_to_either_candidate() {
    let (_dir, root, index) = indexed(&[
        ("source.md", "see [[Target]]\n"),
        ("one/Target.md", "# Target\n"),
        ("two/Target.md", "# Target\n"),
    ]);

    let rows = index.graph(&root).expect("graph");

    assert!(
        rows.edges.is_empty(),
        "a target naming two notes picks neither: {:?}",
        rows.edges
    );
    assert_eq!(rows.nodes.len(), 3, "all three notes are still nodes");
}

#[test]
fn a_pair_linked_more_than_once_is_one_edge_carrying_the_count() {
    let (_dir, root, index) = indexed(&[
        (
            "source.md",
            "[[Target]] again [[Target]]\n\nand once more [[Target]]\n",
        ),
        ("Target.md", "# Target\n"),
    ]);

    let rows = index.graph(&root).expect("graph");

    assert_eq!(rows.edges.len(), 1, "three links, one pair");
    assert_eq!(
        edge_count(&rows, &key(&root, "source.md"), &key(&root, "Target.md")),
        Some(3)
    );
}

#[test]
fn a_note_linking_to_itself_draws_no_edge() {
    let (_dir, root, index) = indexed(&[("Self.md", "# Self\n\nabout [[Self]]\n")]);

    let rows = index.graph(&root).expect("graph");

    assert_eq!(rows.nodes.len(), 1);
    assert!(
        rows.edges.is_empty(),
        "a loop on one node says nothing about the folder: {:?}",
        rows.edges
    );
}

#[test]
fn a_file_that_is_not_a_note_is_not_a_node() {
    let (_dir, root, index) = indexed(&[
        ("Note.md", "# Note\n"),
        ("Long.markdown", "# Long\n"),
        ("notes.txt", "plain text\n"),
        ("more.text", "plain text\n"),
    ]);

    let rows = index.graph(&root).expect("graph");

    let paths = node_paths(&rows);
    assert!(paths.contains(&key(&root, "Note.md").as_str()));
    assert!(
        paths.contains(&key(&root, "Long.markdown").as_str()),
        "a `.markdown` file is a note a link can name"
    );
    assert_eq!(
        rows.nodes.len(),
        2,
        "the two plain-text files are indexed and are not notes: {:?}",
        paths
    );
}

#[test]
fn an_edge_reaching_outside_the_node_set_is_dropped() {
    let (dir, root, index) = indexed(&[
        ("source.md", "see [[Target]]\n"),
        ("Target.md", "# Target\n"),
        ("notes.txt", "plain text\n"),
    ]);

    // A row reaching a file that is indexed and is not a note. The scanner
    // writes none today, and an edge drawn to a node the graph does not carry
    // is the failure this guards, so the row is put in by hand.
    let conn = writ_storage::database::connection::open_database(&dir.path().join("writ.db"))
        .expect("open_database");
    conn.execute(
        "INSERT INTO links (from_path, to_target, to_path, kind, line, col)
         VALUES (?1, 'notes', ?2, 'markdown', 1, 0)",
        rusqlite::params![key(&root, "source.md"), key(&root, "notes.txt")],
    )
    .expect("insert link");
    drop(conn);

    let rows = index.graph(&root).expect("graph");

    let known: Vec<&str> = node_paths(&rows);
    assert!(!rows.edges.is_empty(), "the resolved link is still an edge");
    for edge in &rows.edges {
        assert!(
            known.contains(&edge.from_path.as_str()) && known.contains(&edge.to_path.as_str()),
            "every edge end must be a node: {edge:?}"
        );
    }
}

#[test]
fn folder_is_the_first_segment_under_the_notes_root() {
    let (_dir, root, index) = indexed(&[
        ("Top.md", "# Top\n"),
        ("work/Deep.md", "# Deep\n"),
        ("work/again/Deeper.md", "# Deeper\n"),
    ]);

    let rows = index.graph(&root).expect("graph");

    let folder = |name: &str| {
        let path = key(&root, name);
        rows.nodes
            .iter()
            .find(|node| node.path == path)
            .unwrap_or_else(|| panic!("{name} is not a node"))
            .folder
            .clone()
    };
    assert_eq!(folder("Top.md"), "", "a note in the root has no folder");
    assert_eq!(folder("work/Deep.md"), "work");
    assert_eq!(
        folder("work/again/Deeper.md"),
        "work",
        "the first segment, not the whole path"
    );
}

#[test]
fn a_node_carries_the_name_a_link_calls_it_by() {
    let (_dir, root, index) = indexed(&[("work/Weekly review.md", "# Weekly review\n")]);

    let rows = index.graph(&root).expect("graph");

    assert_eq!(rows.nodes.len(), 1);
    assert_eq!(rows.nodes[0].name, "Weekly review");
}

#[test]
fn an_empty_folder_is_an_empty_graph_rather_than_an_error() {
    let (_dir, root, index) = indexed(&[]);

    let rows = index.graph(&root).expect("graph");

    assert!(rows.nodes.is_empty());
    assert!(rows.edges.is_empty());
}

#[test]
fn edges_are_ordered_so_two_reads_of_one_folder_agree() {
    let (_dir, root, index) = indexed(&[
        ("b.md", "[[a]] [[c]]\n"),
        ("a.md", "[[c]]\n"),
        ("c.md", "# c\n"),
    ]);

    let first = index.graph(&root).expect("graph");
    let second = index.graph(&root).expect("graph");

    assert_eq!(first.nodes, second.nodes);
    assert_eq!(first.edges, second.edges);
    assert_eq!(first.edges.len(), 3);
}
