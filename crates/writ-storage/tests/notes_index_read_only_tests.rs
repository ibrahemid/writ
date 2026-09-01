//! The read surfaces a second process needs: a read-only open, the schema
//! version it compares against its own build, how much of a note the index
//! holds, and the folder's tag list.

use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;
use writ_storage::database::connection::{open_database, open_database_read_only};
use writ_storage::database::migrations::{
    applied_schema_version, binary_schema_version, run_migrations,
};
use writ_storage::notes_index::{self, IndexedBy, NotesIndex, NotesIndexStore};

fn never_cancelled() -> impl Fn() -> bool {
    || false
}

fn never_dataless() -> impl Fn(&Path) -> bool {
    |_| false
}

fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    (dir, db_path, notes)
}

fn write_note(notes: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = notes.join(name);
    std::fs::write(&path, body).expect("write note");
    path
}

fn walk(db_path: &Path, notes: &Path) {
    let conn = open_database(db_path).expect("open_database");
    notes_index::reconcile(&conn, notes, &never_cancelled(), &never_dataless()).expect("reconcile");
}

#[test]
fn a_read_only_connection_reads_the_rows_a_walk_wrote() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "#idea\n\nSee [[Two]].\n");
    write_note(&notes, "Two.md", "body\n");
    walk(&db_path, &notes);

    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    let key = notes_index::index_key(&notes.join("One.md"));
    assert_eq!(store.links_from(&key).expect("links").len(), 1);
}

#[test]
fn a_read_only_connection_refuses_a_write() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "body\n");
    walk(&db_path, &notes);

    let conn = open_database_read_only(&db_path).expect("open_read_only");
    let wrote = conn.execute("DELETE FROM files", []);
    assert!(wrote.is_err(), "a read-only connection deleted rows");
}

#[test]
fn a_read_only_open_of_an_absent_database_fails_and_creates_nothing() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");

    assert!(NotesIndexStore::open_read_only(&db_path).is_err());
    assert!(
        !db_path.exists(),
        "the read-only open created {}",
        db_path.display()
    );
}

#[test]
fn a_migrated_database_reports_the_version_this_binary_embeds() {
    let (_dir, db_path, _notes) = fixture();
    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    assert_eq!(
        store.schema_version().expect("version"),
        binary_schema_version()
    );
}

#[test]
fn a_database_with_no_migration_run_reports_zero() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = Connection::open(&db_path).expect("open");
    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
    )
    .expect("create table");

    assert_eq!(applied_schema_version(&conn).expect("version"), 0);
}

#[test]
fn a_database_with_no_schema_version_table_reports_zero() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = Connection::open(&db_path).expect("open");

    assert_eq!(applied_schema_version(&conn).expect("version"), 0);
}

#[test]
fn a_note_read_for_its_text_is_indexed_by_content() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "body\n");
    walk(&db_path, &notes);

    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    let key = notes_index::index_key(&notes.join("One.md"));
    assert_eq!(
        store.indexed_by(&key).expect("indexed_by"),
        Some(IndexedBy::Content)
    );
}

#[test]
fn a_note_with_no_data_on_this_machine_is_indexed_by_name() {
    let (_dir, db_path, notes) = fixture();
    let placeholder = write_note(&notes, "Away.md", "");
    let conn = open_database(&db_path).expect("open_database");
    let dataless = |path: &Path| path == placeholder;
    notes_index::reconcile(&conn, &notes, &never_cancelled(), &dataless).expect("reconcile");
    drop(conn);

    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    let key = notes_index::index_key(&placeholder);
    assert_eq!(
        store.indexed_by(&key).expect("indexed_by"),
        Some(IndexedBy::Name)
    );
    assert!(
        store.facts(&key).expect("facts").tags.is_empty(),
        "a name-only row cannot carry facts"
    );
}

#[test]
fn a_note_the_index_does_not_hold_is_indexed_by_nothing() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "body\n");
    walk(&db_path, &notes);

    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    let key = notes_index::index_key(&notes.join("Never.md"));
    assert_eq!(store.indexed_by(&key).expect("indexed_by"), None);
}

#[test]
fn the_tag_list_counts_notes_not_mentions() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "#idea and #idea again\n\n#draft\n");
    write_note(&notes, "Two.md", "#idea\n");
    write_note(&notes, "Three.md", "#idea\n");
    walk(&db_path, &notes);

    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    assert_eq!(
        store.all_tags().expect("all_tags"),
        vec![("idea".to_string(), 3), ("draft".to_string(), 1)]
    );
}

#[test]
fn a_folder_with_no_tags_lists_none() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "body\n");
    walk(&db_path, &notes);

    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    assert!(store.all_tags().expect("all_tags").is_empty());
}

#[test]
fn tags_with_the_same_note_count_are_listed_in_tag_order() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "#zebra #apple #mango\n");
    walk(&db_path, &notes);

    let store = NotesIndexStore::open_read_only(&db_path).expect("open_read_only");
    let tags: Vec<String> = store
        .all_tags()
        .expect("all_tags")
        .into_iter()
        .map(|(tag, _)| tag)
        .collect();
    assert_eq!(tags, vec!["apple", "mango", "zebra"]);
}

#[test]
fn indexed_by_reads_back_every_value_the_column_can_hold() {
    assert_eq!(IndexedBy::from_stored("content"), IndexedBy::Content);
    assert_eq!(IndexedBy::from_stored("name"), IndexedBy::Name);
    assert_eq!(IndexedBy::from_stored(""), IndexedBy::Content);
    assert_eq!(IndexedBy::Content.as_str(), "content");
    assert_eq!(IndexedBy::Name.as_str(), "name");
}

#[test]
fn the_tag_list_is_reachable_without_the_store_wrapper() {
    let (_dir, db_path, notes) = fixture();
    write_note(&notes, "One.md", "#idea\n");
    walk(&db_path, &notes);

    let conn = open_database_read_only(&db_path).expect("open_read_only");
    let index = NotesIndex::new(&conn);
    assert_eq!(
        index.all_tags().expect("all_tags"),
        vec![("idea".to_string(), 1)]
    );
}
