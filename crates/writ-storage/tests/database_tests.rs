use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

fn setup_temp_db() -> (TempDir, rusqlite::Connection) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("failed to open database");
    (dir, conn)
}

#[test]
fn open_database_creates_file_in_wal_mode() {
    let (_dir, conn) = setup_temp_db();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("failed to query journal_mode");
    assert_eq!(mode, "wal");
}

#[test]
fn run_migrations_creates_schema() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    let tables: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .expect("failed to prepare statement");
        stmt.query_map([], |row| row.get(0))
            .expect("query failed")
            .map(|r| r.expect("row error"))
            .collect()
    };

    assert!(tables.contains(&"buffers".to_string()));
    assert!(tables.contains(&"session_snapshots".to_string()));
    assert!(tables.contains(&"schema_version".to_string()));

    let fts_tables: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='buffer_fts'")
            .expect("failed to prepare statement");
        stmt.query_map([], |row| row.get(0))
            .expect("query failed")
            .map(|r| r.expect("row error"))
            .collect()
    };

    assert!(fts_tables.contains(&"buffer_fts".to_string()));
}

#[test]
fn run_migrations_is_idempotent() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("first migration failed");
    run_migrations(&conn).expect("second migration failed");
}

#[test]
fn schema_version_is_tracked() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    let max_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .expect("failed to query schema_version");

    assert!(max_version >= 1);
}
