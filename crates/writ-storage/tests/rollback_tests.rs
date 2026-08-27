use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;
use writ_storage::rollback::{
    age_out_rollback_copy, write_rollback_copy, ROLLBACK_COPY_SUFFIX, ROLLBACK_KEEP_LAUNCHES,
};
use writ_storage::schema_meta;

/// A database with a committed write still in its write-ahead log, so the
/// `-wal` file exists beside it and the copy has to reach through it.
fn setup() -> (TempDir, PathBuf, rusqlite::Connection) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    insert_file(&conn, "/notes/one.md");
    assert!(
        with_suffix(&db_path, "-wal").exists(),
        "the write-ahead log should exist before the copy is written"
    );
    (dir, db_path, conn)
}

fn insert_file(conn: &rusqlite::Connection, path: &str) {
    conn.execute(
        "INSERT INTO files (path, size, mtime, hash, indexed_at)
         VALUES (?1, 3, 1, 'abc', '2026-08-28T00:00:00Z')",
        [path],
    )
    .expect("failed to insert row");
}

fn file_paths(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT path FROM files ORDER BY path")
        .expect("failed to prepare statement");
    let paths = stmt
        .query_map([], |row| row.get(0))
        .expect("query failed")
        .map(|r| r.expect("row error"))
        .collect();
    paths
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[test]
fn rollback_copy_is_written_beside_the_database() {
    let (_dir, db_path, conn) = setup();

    let copy = write_rollback_copy(&conn, &db_path).expect("failed to write the copy");

    assert_eq!(copy.parent(), db_path.parent());
    assert_eq!(
        copy.file_name().expect("the copy has no name"),
        format!("writ.db{ROLLBACK_COPY_SUFFIX}").as_str()
    );
    assert!(copy.exists());
    assert!(
        !with_suffix(&copy, "-wal").exists(),
        "the copy is one self-contained file"
    );
    assert!(!with_suffix(&copy, "-shm").exists());
}

#[test]
fn a_copy_taken_with_uncheckpointed_wal_pages_is_complete() {
    let (_dir, db_path, conn) = setup();
    insert_file(&conn, "/notes/two.md");
    insert_file(&conn, "/notes/three.md");
    assert!(
        with_suffix(&db_path, "-wal")
            .metadata()
            .expect("failed to stat the write-ahead log")
            .len()
            > 0,
        "the rows under test should still be in the log, not checkpointed"
    );

    let copy = write_rollback_copy(&conn, &db_path).expect("failed to write the copy");

    let copied = open_database(&copy).expect("the copy does not open");
    assert_eq!(
        file_paths(&copied),
        vec![
            "/notes/one.md".to_string(),
            "/notes/three.md".to_string(),
            "/notes/two.md".to_string(),
        ]
    );
}

#[test]
fn restoring_the_copy_by_rename_yields_the_pre_migration_database() {
    let (_dir, db_path, conn) = setup();
    let copy = write_rollback_copy(&conn, &db_path).expect("failed to write the copy");

    insert_file(&conn, "/notes/after-the-migration.md");
    drop(conn);

    std::fs::remove_file(with_suffix(&db_path, "-wal")).ok();
    std::fs::remove_file(with_suffix(&db_path, "-shm")).ok();
    std::fs::rename(&copy, &db_path).expect("failed to rename the copy over the database");

    let restored = open_database(&db_path).expect("the restored database does not open");
    assert_eq!(file_paths(&restored), vec!["/notes/one.md".to_string()]);
}

#[test]
fn rollback_copy_is_recorded_in_schema_meta() {
    let (_dir, db_path, conn) = setup();

    let copy = write_rollback_copy(&conn, &db_path).expect("failed to write the copy");

    let recorded = schema_meta::get(&conn, schema_meta::KEY_ROLLBACK_COPY_PATH)
        .expect("get failed")
        .expect("the copy path was not recorded");
    assert_eq!(recorded, copy.to_string_lossy());

    let launches = schema_meta::get(&conn, schema_meta::KEY_ROLLBACK_COPY_LAUNCHES)
        .expect("get failed")
        .expect("the launch count was not recorded");
    assert_eq!(launches, "0");

    let stamp: String = conn
        .query_row(
            "SELECT updated_at FROM schema_meta WHERE key = ?1",
            [schema_meta::KEY_ROLLBACK_COPY_PATH],
            |row| row.get(0),
        )
        .expect("failed to read the stamp");
    chrono::DateTime::parse_from_rfc3339(&stamp).expect("the stamp is not an RFC 3339 timestamp");
}

#[test]
fn rollback_copy_is_not_rewritten_on_a_second_call() {
    let (_dir, db_path, conn) = setup();

    let copy = write_rollback_copy(&conn, &db_path).expect("failed to write the copy");
    std::fs::write(&copy, b"kept").expect("failed to overwrite the copy");
    insert_file(&conn, "/notes/two.md");

    let again = write_rollback_copy(&conn, &db_path).expect("the second call failed");

    assert_eq!(again, copy);
    assert_eq!(
        std::fs::read(&copy).expect("failed to read the copy"),
        b"kept"
    );
}

#[test]
fn a_copy_cannot_be_taken_inside_a_transaction() {
    let (_dir, db_path, conn) = setup();
    conn.execute_batch("BEGIN")
        .expect("failed to open a transaction");

    let result = write_rollback_copy(&conn, &db_path);

    assert!(
        matches!(result, Err(StorageError::RollbackCopyInTransaction)),
        "expected RollbackCopyInTransaction, got {result:?}"
    );
    assert!(!with_suffix(&db_path, ROLLBACK_COPY_SUFFIX).exists());
    conn.execute_batch("COMMIT").expect("failed to commit");
}

#[test]
fn rollback_copy_is_deleted_after_ten_launches() {
    let (_dir, db_path, conn) = setup();
    let copy = write_rollback_copy(&conn, &db_path).expect("failed to write the copy");

    for launch in 1..ROLLBACK_KEEP_LAUNCHES {
        let deleted = age_out_rollback_copy(&conn, ROLLBACK_KEEP_LAUNCHES).expect("age out failed");
        assert!(!deleted, "launch {launch} must keep the copy");
        assert!(copy.exists());
    }

    let deleted = age_out_rollback_copy(&conn, ROLLBACK_KEEP_LAUNCHES).expect("age out failed");

    assert!(deleted);
    assert!(!copy.exists());
    assert_eq!(
        schema_meta::get(&conn, schema_meta::KEY_ROLLBACK_COPY_PATH).expect("get failed"),
        None
    );
    assert_eq!(
        schema_meta::get(&conn, schema_meta::KEY_ROLLBACK_COPY_LAUNCHES).expect("get failed"),
        None
    );
    assert!(
        !age_out_rollback_copy(&conn, ROLLBACK_KEEP_LAUNCHES).expect("age out failed"),
        "a second age out has nothing to delete"
    );
    assert_eq!(ROLLBACK_KEEP_LAUNCHES, 10);
}

#[test]
fn a_launch_count_that_is_not_a_number_is_an_error() {
    let (_dir, db_path, conn) = setup();
    let copy = write_rollback_copy(&conn, &db_path).expect("failed to write the copy");
    schema_meta::set(&conn, schema_meta::KEY_ROLLBACK_COPY_LAUNCHES, "many").expect("set failed");

    let result = age_out_rollback_copy(&conn, ROLLBACK_KEEP_LAUNCHES);

    match result {
        Err(StorageError::SchemaMetaValue { key, value }) => {
            assert_eq!(key, schema_meta::KEY_ROLLBACK_COPY_LAUNCHES);
            assert_eq!(value, "many");
        }
        other => panic!("expected SchemaMetaValue, got {other:?}"),
    }
    assert!(copy.exists(), "a corrupt count must not delete the copy");
}
