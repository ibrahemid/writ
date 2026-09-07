//! A database that lost its notes-index tables while its schema version stayed
//! current.
//!
//! Deleting the whole file is covered next door in `notes_index_tests`, and it
//! is a different case: an absent database runs every migration on the way back
//! up. Here the file is there, `schema_version` says 43, and the tables the
//! index derives are gone, which the version-gated runner recreates nothing
//! for. What is tested is that the folder still answers the same afterwards.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::index_repair::{
    repair_notes_index, IndexRepairOutcome, DERIVED_OBJECTS, FTS_OBJECT, SCHEMA_META_OBJECT,
};
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{
    BacklinkRow, GraphRows, HeadingRow, NoteFactsRow, NotesIndexStore,
};
use writ_storage::schema_meta;

/// The folder as it was written in the other editor, the fixture
/// `obsidian_folder_tests` walks.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/obsidian-folder"
);

/// Copies `from` to `to`, dot-named entries included: the fixture's settings
/// tree and trash both start with a dot, and what they contribute to the
/// snapshot is exactly that they contribute nothing.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create folder");
    for entry in std::fs::read_dir(from).expect("read folder") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

/// Everything the index derived from one note.
#[derive(Debug, PartialEq, Eq)]
struct NoteSnapshot {
    path: String,
    facts: NoteFactsRow,
    headings: Vec<HeadingRow>,
    backlinks: Vec<BacklinkRow>,
}

/// Everything the index derived from the folder, in one order so two reads
/// compare.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    notes: Vec<NoteSnapshot>,
    tags: Vec<(String, usize)>,
    graph: GraphRows,
}

fn snapshot(index: &NotesIndexStore, root: &Path) -> Snapshot {
    let mut paths = index.note_paths().expect("note paths");
    paths.sort();
    let notes = paths
        .into_iter()
        .map(|path| {
            let facts = index.facts(&path).expect("facts");
            NoteSnapshot {
                headings: facts.headings.clone(),
                backlinks: index.backlinks(&path).expect("backlinks"),
                facts,
                path,
            }
        })
        .collect();
    Snapshot {
        notes,
        tags: index.all_tags().expect("all_tags"),
        graph: index.graph(root).expect("graph"),
    }
}

/// The fixture folder, copied out, indexed once, and the database closed.
fn indexed() -> (TempDir, PathBuf, PathBuf, Snapshot) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);

    let root = dir.path().join("notes");
    copy_tree(Path::new(FIXTURE), &root);

    let index = NotesIndexStore::open(&db_path).expect("index");
    index
        .reconcile(&root, &|| false, &|_| false)
        .expect("reconcile");
    let before = snapshot(&index, &root);
    drop(index);

    assert!(
        !before.tags.is_empty() && !before.graph.edges.is_empty(),
        "the fixture has to derive something for its return to mean anything"
    );
    assert!(
        before
            .notes
            .iter()
            .any(|note| note.backlinks.iter().any(|row| !row.context.is_empty())),
        "a backlink quotes the text the full-text table holds, so the snapshot \
         covers files_fts as well"
    );

    (dir, root, db_path, before)
}

/// Reconciles the folder again through a fresh store and reads it back.
fn reconciled(db_path: &Path, root: &Path) -> Snapshot {
    let index = NotesIndexStore::open(db_path).expect("reopen index");
    index
        .reconcile(root, &|| false, &|_| false)
        .expect("reconcile after the repair");
    snapshot(&index, root)
}

fn object_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .expect("sqlite_master")
        > 0
}

fn applied_version(conn: &Connection) -> i32 {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )
    .expect("schema_version")
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {}", table), [], |row| {
        row.get(0)
    })
    .expect("count")
}

fn drop_tables(db_path: &Path, tables: &[&str]) {
    let conn = open_database(db_path).expect("open_database");
    for table in tables {
        conn.execute_batch(&format!("DROP TABLE {};", table))
            .expect("drop table");
    }
}

#[test]
fn a_hole_beside_a_missing_meta_table_still_leaves_a_database_that_opens() {
    let (_dir, root, db_path, before) = indexed();

    // The two are created by one migration, so the corruption that takes one
    // plausibly takes the other. The census the repair clears is written to
    // schema_meta, which is what would fail the repair and, through it, the
    // launch.
    drop_tables(&db_path, &["links", SCHEMA_META_OBJECT]);

    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("a database that lost both still opens");
    assert!(object_exists(&conn, SCHEMA_META_OBJECT));
    for name in DERIVED_OBJECTS {
        assert!(object_exists(&conn, name), "{} was not recreated", name);
    }
    drop(conn);

    assert_eq!(reconciled(&db_path, &root), before);
}

#[test]
fn a_missing_meta_table_on_its_own_is_recreated() {
    let (_dir, root, db_path, before) = indexed();

    drop_tables(&db_path, &[SCHEMA_META_OBJECT]);

    let conn = open_database(&db_path).expect("open_database");
    assert_eq!(
        repair_notes_index(&conn).expect("repair"),
        IndexRepairOutcome::Repaired,
        "every reconcile reads the census, so the table it lives in is a hole \
         like any other"
    );
    assert!(object_exists(&conn, SCHEMA_META_OBJECT));
    drop(conn);

    assert_eq!(reconciled(&db_path, &root), before);
}

#[test]
fn a_database_without_files_names_what_it_cannot_repair() {
    let (_dir, _root, db_path, _before) = indexed();

    drop_tables(&db_path, &["links", "files"]);

    let conn = open_database(&db_path).expect("open_database");
    assert_eq!(
        repair_notes_index(&conn).expect("repair"),
        IndexRepairOutcome::Unrepairable {
            missing: vec!["files".to_string()],
        },
        "an index around a hole is not an index, and the name says which hole"
    );
    run_migrations(&conn).expect("the launch carries on over a broken database");
    assert!(
        !object_exists(&conn, "links"),
        "nothing was built on top of the missing table"
    );
}

#[test]
fn dropping_the_derived_tables_and_reconciling_brings_every_fact_back() {
    let (_dir, root, db_path, before) = indexed();

    let conn = open_database(&db_path).expect("open_database");
    let version = applied_version(&conn);
    let files = count(&conn, "files");
    drop(conn);

    drop_tables(&db_path, &["links", "properties", "tags", "headings"]);

    let conn = open_database(&db_path).expect("open_database");
    assert_eq!(
        applied_version(&conn),
        version,
        "dropping the tables leaves the recorded version where it was, which is \
         why the runner recreates nothing on its own"
    );
    assert!(
        object_exists(&conn, "files") && object_exists(&conn, FTS_OBJECT),
        "this is corruption of what the index derived, not of what it walked"
    );
    run_migrations(&conn).expect("migrations over the dropped tables");
    for name in DERIVED_OBJECTS {
        assert!(object_exists(&conn, name), "{} was not recreated", name);
    }
    assert_eq!(
        applied_version(&conn),
        version,
        "the repair is not a migration and records no version of its own"
    );
    assert_eq!(
        count(&conn, "files"),
        files,
        "the walk's own record is untouched: the repair recreates what is \
         derived from it"
    );
    drop(conn);

    assert_eq!(reconciled(&db_path, &root), before);
}

#[test]
fn one_missing_table_is_enough_to_trigger_the_repair() {
    let (_dir, root, db_path, before) = indexed();

    drop_tables(&db_path, &["tags"]);

    let conn = open_database(&db_path).expect("open_database");
    assert_eq!(
        repair_notes_index(&conn).expect("repair"),
        IndexRepairOutcome::Repaired,
        "one hole is a hole"
    );
    assert_eq!(
        repair_notes_index(&conn).expect("second repair"),
        IndexRepairOutcome::Intact,
        "and it is filled once"
    );
    drop(conn);

    assert_eq!(reconciled(&db_path, &root), before);
}

#[test]
fn a_missing_index_is_a_hole_like_any_other() {
    let (_dir, root, db_path, before) = indexed();

    let conn = open_database(&db_path).expect("open_database");
    conn.execute_batch("DROP INDEX idx_tags_tag;")
        .expect("drop index");
    assert_eq!(
        repair_notes_index(&conn).expect("repair"),
        IndexRepairOutcome::Repaired,
        "a table left without its index answers slowly and silently, so the \
         index counts as missing too"
    );
    assert!(object_exists(&conn, "idx_tags_tag"));
    drop(conn);

    assert_eq!(reconciled(&db_path, &root), before);
}

#[test]
fn a_missing_full_text_table_is_recreated_without_dropping_the_derived_rows() {
    let (_dir, root, db_path, before) = indexed();

    let conn = open_database(&db_path).expect("open_database");
    let links = count(&conn, "links");
    drop(conn);

    drop_tables(&db_path, &[FTS_OBJECT]);

    let conn = open_database(&db_path).expect("open_database");
    assert_eq!(
        repair_notes_index(&conn).expect("repair"),
        IndexRepairOutcome::Repaired
    );
    assert!(object_exists(&conn, FTS_OBJECT), "the shadow is back");
    assert_eq!(
        count(&conn, "links"),
        links,
        "the derived tables are not dropped for a hole that is not theirs"
    );
    drop(conn);

    assert_eq!(reconciled(&db_path, &root), before);
}

#[test]
fn a_healthy_database_is_left_alone() {
    let (_dir, root, db_path, before) = indexed();

    let conn = open_database(&db_path).expect("open_database");
    let census = schema_meta::get(&conn, schema_meta::KEY_NOTES_FACTS_CENSUS)
        .expect("census")
        .expect("a complete pass records one");
    let rows = count(&conn, "links");

    assert_eq!(
        repair_notes_index(&conn).expect("repair"),
        IndexRepairOutcome::Intact
    );
    run_migrations(&conn).expect("migrations over an intact database");

    assert_eq!(
        schema_meta::get(&conn, schema_meta::KEY_NOTES_FACTS_CENSUS).expect("census"),
        Some(census),
        "the census is what sends the walk over every file again: an intact \
         database keeps it, and reads nothing on its next pass"
    );
    assert_eq!(count(&conn, "links"), rows, "no row was recreated away");
    drop(conn);

    let index = NotesIndexStore::open(&db_path).expect("reopen index");
    assert_eq!(snapshot(&index, &root), before);
}

#[test]
fn a_fresh_migration_creates_every_object_the_repair_looks_for() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");

    for name in DERIVED_OBJECTS.iter().chain(std::iter::once(&FTS_OBJECT)) {
        assert!(
            object_exists(&conn, name),
            "{} is in the list the repair checks but not in the schema it \
             checks against, so every launch would rebuild the index",
            name
        );
    }
    assert_eq!(
        repair_notes_index(&conn).expect("repair"),
        IndexRepairOutcome::Intact
    );
}
