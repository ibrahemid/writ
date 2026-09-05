//! Which note a stored link target names (ADR-034).
//!
//! `links.to_target` is written from a target the scanner already parsed, so
//! every reader of that column reads it back through
//! `writ_core::notes::links::stored_target` and never parses it again. A
//! second parse takes another note extension off, which matters for exactly
//! one shape — a target written as `[[Note.md.md]]` — and matters twice over
//! because the pass that re-resolves runs after the pass that writes and
//! overwrites it.
//!
//! The name rule itself is tested in `writ_core::notes::links`. What is tested
//! here is the column: what a walk stores, what an arriving note fills in, and
//! that a row an older build wrote is corrected rather than written back.

use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;
use writ_storage::notes_index::{self, BacklinkCertainty, NotesIndex, NotesIndexStore};

fn never_cancelled() -> impl Fn() -> bool {
    || false
}

fn never_dataless() -> impl Fn(&Path) -> bool {
    |_| false
}

fn fixture() -> (TempDir, Connection, std::path::PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open_database");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    (dir, conn, notes)
}

fn write_note(notes: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = notes.join(name);
    std::fs::write(&path, body).expect("write note");
    path
}

fn walk(conn: &Connection, notes: &Path) {
    notes_index::reconcile(conn, notes, &never_cancelled(), &never_dataless()).expect("reconcile");
}

/// `to_path` for the one link the note at `from` holds.
fn stored_target_of(conn: &Connection, from: &Path) -> Option<String> {
    let links = NotesIndex::new(conn)
        .links_from(&notes_index::index_key(from))
        .expect("links from");
    assert_eq!(links.len(), 1, "the fixture note holds one link");
    assert_eq!(
        links[0].to_target, "Note.md",
        "one extension comes off once"
    );
    links[0].to_path.clone()
}

/// The notes that link to `path`, with how certain each link is.
fn backlink_certainties(conn: &Connection, path: &Path) -> Vec<BacklinkCertainty> {
    NotesIndex::new(conn)
        .backlinks(&notes_index::index_key(path))
        .expect("backlinks")
        .into_iter()
        .map(|row| row.certainty)
        .collect()
}

#[test]
fn a_target_written_with_two_extensions_names_the_note_that_carries_both() {
    let (_dir, conn, notes) = fixture();
    let note = write_note(&notes, "Note.md.md", "# Note\n");
    let from = write_note(&notes, "From.md", "see [[Note.md.md]]\n");
    walk(&conn, &notes);

    assert_eq!(
        stored_target_of(&conn, &from).as_deref(),
        Some(notes_index::index_key(&note).as_str()),
        "the only note answering to the target is the one the link opens"
    );
    assert_eq!(
        backlink_certainties(&conn, &note),
        vec![BacklinkCertainty::Resolved],
        "and it holds the backlink"
    );
}

#[test]
fn a_target_two_notes_answer_to_belongs_to_both_and_resolves_to_neither() {
    let (_dir, conn, notes) = fixture();
    let short = write_note(&notes, "Note.md", "# Short\n");
    let long = write_note(&notes, "Note.md.md", "# Long\n");
    let from = write_note(&notes, "From.md", "see [[Note.md.md]]\n");
    walk(&conn, &notes);

    assert_eq!(
        stored_target_of(&conn, &from),
        None,
        "an ambiguous target is not a link to one of its candidates"
    );
    // The editor asks which note is meant, so the link sits in both lists
    // rather than being a confident backlink of the note it does not open.
    assert_eq!(
        backlink_certainties(&conn, &short),
        vec![BacklinkCertainty::Ambiguous]
    );
    assert_eq!(
        backlink_certainties(&conn, &long),
        vec![BacklinkCertainty::Ambiguous]
    );
}

#[test]
fn a_walk_corrects_a_row_an_older_build_stored_and_leaves_it_corrected() {
    let (_dir, conn, notes) = fixture();
    let note = write_note(&notes, "Note.md.md", "# Note\n");
    let from = write_note(&notes, "From.md", "see [[Note.md.md]]\n");
    walk(&conn, &notes);

    // What a build that read the stored target through the whole target parser
    // left behind: a second extension off, and the name of a note that is not
    // even indexed here.
    conn.execute("UPDATE links SET to_path = 'Note.md'", [])
        .expect("plant the old answer");

    walk(&conn, &notes);
    let key = notes_index::index_key(&note);
    assert_eq!(
        stored_target_of(&conn, &from).as_deref(),
        Some(key.as_str()),
        "a walk re-resolves every row, so the old answer is corrected"
    );

    walk(&conn, &notes);
    assert_eq!(
        stored_target_of(&conn, &from).as_deref(),
        Some(key.as_str()),
        "and the next walk does not write it back"
    );
}

#[test]
fn the_note_arriving_fills_in_the_link_that_named_it_with_both_extensions() {
    let (dir, _conn, notes) = fixture();
    let store = NotesIndexStore::open(&dir.path().join("writ.db")).expect("store");
    let from = write_note(&notes, "From.md", "see [[Note.md.md]]\n");
    store.index_path(&from).expect("index from");

    let key = notes_index::index_key(&from);
    assert_eq!(
        store.links_from(&key).expect("links")[0].to_path,
        None,
        "nothing answers to the target yet"
    );

    // A save revisits only the links naming the saved note, and those keys
    // carry the extension the target keeps.
    let note = write_note(&notes, "Note.md.md", "# Note\n");
    store.index_path(&note).expect("index note");

    assert_eq!(
        store.links_from(&key).expect("links")[0].to_path.as_deref(),
        Some(notes_index::index_key(&note).as_str()),
        "the arriving note fills in the link that waited for it"
    );
}
