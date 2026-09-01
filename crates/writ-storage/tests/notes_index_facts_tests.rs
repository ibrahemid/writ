//! The four tables derived from a note's text: `links`, `properties`, `tags`
//! and `headings` (ADR-034).
//!
//! The policy they are derived by is tested in `writ_core::notes`. What is
//! tested here is the storage half: that both writers fill them, that a link
//! written before its target exists is filled in when the target arrives, that
//! an ambiguous target stores no target at all, and that emptying the four
//! tables and reconciling brings them back.

use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;
use writ_core::notes::links::Resolution;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, NotesIndex, NotesIndexStore};

fn never_cancelled() -> impl Fn() -> bool {
    || false
}

fn never_dataless() -> impl Fn(&Path) -> bool {
    |_| false
}

fn fixture() -> (TempDir, Connection, std::path::PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    (dir, conn, notes)
}

fn write_note(notes: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = notes.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, body).expect("write note");
    path
}

fn walk(conn: &Connection, notes: &Path) {
    notes_index::reconcile(conn, notes, &never_cancelled(), &never_dataless()).expect("reconcile");
}

fn counts(conn: &Connection) -> (i64, i64, i64, i64) {
    let one = |table: &str| {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("count")
    };
    (
        one("links"),
        one("properties"),
        one("tags"),
        one("headings"),
    )
}

#[test]
fn a_walk_fills_all_four_derived_tables() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(
        &notes,
        "note.md",
        "---\ntitle: Weekly\ntags: [a, b]\n---\n# Heading\n\n#inbox and [[Other]]\n",
    );
    write_note(&notes, "Other.md", "the other note\n");
    walk(&conn, &notes);

    let key = notes_index::index_key(&path);
    let facts = NotesIndex::new(&conn).facts(&key).expect("facts");
    assert_eq!(facts.properties.len(), 2);
    assert_eq!(facts.tags, vec![("inbox".to_string(), 7)]);
    assert_eq!(facts.headings.len(), 1);
    assert_eq!(facts.headings[0].slug, "heading");
    assert_eq!(facts.links.len(), 1);
    assert_eq!(facts.links[0].to_target, "Other");
    assert_eq!(
        facts.links[0].to_path.as_deref(),
        Some(notes_index::index_key(&notes.join("Other.md")).as_str())
    );
}

#[test]
fn the_watchers_single_file_write_fills_them_too() {
    let (dir, _conn, notes) = fixture();
    let store = NotesIndexStore::open(&dir.path().join("writ.db")).expect("store");
    let target = write_note(&notes, "Target.md", "# Target\n");
    let path = write_note(&notes, "source.md", "see [[Target]] #idea\n");

    assert!(store.index_path(&target).expect("index target"));
    assert!(store.index_path(&path).expect("index source"));

    let key = notes_index::index_key(&path);
    let links = store.links_from(&key).expect("links from");
    assert_eq!(links.len(), 1);
    assert_eq!(
        links[0].to_path.as_deref(),
        Some(notes_index::index_key(&target).as_str())
    );
    assert_eq!(store.facts(&key).expect("facts").tags.len(), 1);
}

#[test]
fn a_link_written_before_its_target_is_filled_in_when_the_target_arrives() {
    let (dir, _conn, notes) = fixture();
    let store = NotesIndexStore::open(&dir.path().join("writ.db")).expect("store");
    let source = write_note(&notes, "source.md", "waiting on [[Later]]\n");
    store.index_path(&source).expect("index source");

    let key = notes_index::index_key(&source);
    assert_eq!(
        store.links_from(&key).expect("links")[0].to_path,
        None,
        "a target that does not exist yet resolves to nothing"
    );

    let later = write_note(&notes, "Later.md", "here now\n");
    store.index_path(&later).expect("index later");

    let target_key = notes_index::index_key(&later);
    assert_eq!(
        store.links_from(&key).expect("links")[0].to_path.as_deref(),
        Some(target_key.as_str()),
        "indexing the target backfills the link that named it"
    );
    assert_eq!(
        store
            .links_to(&target_key)
            .expect("links to")
            .into_iter()
            .map(|link| link.from_path)
            .collect::<Vec<_>>(),
        vec![key]
    );
}

#[test]
fn a_link_to_a_note_that_is_gone_stops_pointing_at_it() {
    let (_dir, conn, notes) = fixture();
    let source = write_note(&notes, "source.md", "see [[Gone]]\n");
    let gone = write_note(&notes, "Gone.md", "still here\n");
    walk(&conn, &notes);

    let key = notes_index::index_key(&source);
    assert!(NotesIndex::new(&conn).links_from(&key).expect("links")[0]
        .to_path
        .is_some());

    std::fs::remove_file(&gone).expect("remove");
    walk(&conn, &notes);
    assert_eq!(
        NotesIndex::new(&conn).links_from(&key).expect("links")[0].to_path,
        None,
        "a link must not keep pointing at a row the walk removed"
    );
}

#[test]
fn the_watchers_delete_arm_empties_the_links_that_pointed_at_the_note() {
    let (dir, _conn, notes) = fixture();
    let store = NotesIndexStore::open(&dir.path().join("writ.db")).expect("store");
    let target = write_note(&notes, "Target.md", "# Target\n");
    let source = write_note(&notes, "source.md", "see [[Target]]\n");
    assert!(store.index_path(&target).expect("index target"));
    assert!(store.index_path(&source).expect("index source"));

    let key = notes_index::index_key(&source);
    assert!(store.links_from(&key).expect("links")[0].to_path.is_some());

    std::fs::remove_file(&target).expect("remove");
    store.forget_path(&target).expect("forget");

    assert_eq!(
        store.links_from(&key).expect("links")[0].to_path,
        None,
        "a deleted note must not leave links resolved to it until the next walk"
    );
}

#[test]
fn deleting_one_of_two_notes_of_the_same_name_resolves_the_link_to_the_other() {
    let (dir, _conn, notes) = fixture();
    let store = NotesIndexStore::open(&dir.path().join("writ.db")).expect("store");
    let first = write_note(&notes, "a/Note.md", "one\n");
    let second = write_note(&notes, "b/Note.md", "two\n");
    let source = write_note(&notes, "source.md", "see [[Note]]\n");
    for path in [&first, &second, &source] {
        assert!(store.index_path(path).expect("index"));
    }

    let key = notes_index::index_key(&source);
    assert_eq!(
        store.links_from(&key).expect("links")[0].to_path,
        None,
        "two notes of that name is ambiguous, and ambiguous stores nothing"
    );

    std::fs::remove_file(&first).expect("remove");
    store.forget_path(&first).expect("forget");

    assert_eq!(
        store.links_from(&key).expect("links")[0].to_path.as_deref(),
        Some(notes_index::index_key(&second).as_str()),
        "one candidate left is no longer ambiguous"
    );
}

#[test]
fn an_ambiguous_target_stores_no_target_and_reports_both_notes() {
    let (_dir, conn, notes) = fixture();
    let source = write_note(&notes, "from/source.md", "see [[Note]]\n");
    write_note(&notes, "a/Note.md", "one\n");
    write_note(&notes, "b/Note.md", "two\n");
    walk(&conn, &notes);

    let key = notes_index::index_key(&source);
    let index = NotesIndex::new(&conn);
    assert_eq!(
        index.links_from(&key).expect("links")[0].to_path,
        None,
        "an ambiguous target is never stored as one of the candidates"
    );

    let Resolution::Ambiguous(candidates) = index.resolve_link(&key, "Note").expect("resolve")
    else {
        panic!("two notes of one name at the same depth are ambiguous");
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        candidates,
        index.candidate_paths("Note").expect("candidates")
    );
}

#[test]
fn a_resolved_target_reports_the_line_of_the_heading_it_names() {
    let (_dir, conn, notes) = fixture();
    let source = write_note(&notes, "source.md", "see [[Target#Second Part]]\n");
    let target = write_note(&notes, "Target.md", "# First\n\n## Second Part\n");
    walk(&conn, &notes);

    let index = NotesIndex::new(&conn);
    let key = notes_index::index_key(&source);
    let target_key = notes_index::index_key(&target);
    assert_eq!(
        index
            .resolve_link(&key, "Target#Second Part")
            .expect("resolve"),
        Resolution::Resolved(target_key.clone())
    );
    assert_eq!(
        index
            .heading_line(&target_key, "second-part")
            .expect("heading line"),
        Some(3)
    );
    assert_eq!(
        index
            .heading_line(&target_key, "no-such-heading")
            .expect("heading line"),
        None,
        "a heading the note does not have leaves the file resolved"
    );
}

#[test]
fn emptying_the_four_tables_and_reconciling_rebuilds_them() {
    let (_dir, conn, notes) = fixture();
    write_note(
        &notes,
        "one.md",
        "---\ntitle: One\n---\n# One\n\n#tag [[two]]\n",
    );
    write_note(&notes, "two.md", "# Two\n\n#other\n");
    walk(&conn, &notes);

    let before = counts(&conn);
    assert!(before.0 > 0 && before.1 > 0 && before.2 > 0 && before.3 > 0);

    for table in ["links", "properties", "tags", "headings"] {
        conn.execute(&format!("DELETE FROM {table}"), [])
            .expect("empty table");
    }
    assert_eq!(counts(&conn), (0, 0, 0, 0));

    walk(&conn, &notes);
    assert_eq!(
        counts(&conn),
        before,
        "reconcile must rebuild what it derived"
    );
}

#[test]
fn a_walk_that_changed_nothing_does_not_rebuild_a_folder_with_no_facts_in_it() {
    let (_dir, conn, notes) = fixture();
    write_note(&notes, "plain.md", "just a sentence\n");
    write_note(&notes, "other.md", "another sentence\n");
    walk(&conn, &notes);
    assert_eq!(counts(&conn), (0, 0, 0, 0));

    let second =
        notes_index::reconcile(&conn, &notes, &never_cancelled(), &never_dataless()).expect("walk");
    assert_eq!(
        (second.added, second.updated, second.removed),
        (0, 0, 0),
        "an empty set of facts is a real answer, not a reason to re-read every file"
    );
}

#[test]
fn a_note_that_was_never_read_has_no_facts() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "evicted.md", "# Heading\n\n#tag\n");
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &|candidate| {
        candidate
            .file_name()
            .is_some_and(|name| name == "evicted.md")
    })
    .expect("reconcile");

    let facts = NotesIndex::new(&conn)
        .facts(&notes_index::index_key(&path))
        .expect("facts");
    assert_eq!(facts.headings.len(), 0);
    assert_eq!(facts.tags.len(), 0);
    assert_eq!(
        facts.links.len(),
        0,
        "a placeholder is indexed by name, and a name says nothing about links"
    );
}

#[test]
fn removing_a_note_takes_its_derived_rows_with_it() {
    let (_dir, conn, notes) = fixture();
    let path = write_note(&notes, "doomed.md", "# Doomed\n\n#tag [[nowhere]]\n");
    walk(&conn, &notes);
    assert_ne!(counts(&conn), (0, 0, 0, 0));

    std::fs::remove_file(&path).expect("remove");
    walk(&conn, &notes);
    assert_eq!(counts(&conn), (0, 0, 0, 0));
}
