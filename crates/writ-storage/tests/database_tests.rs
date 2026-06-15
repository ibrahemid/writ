use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;

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

#[test]
fn refuses_to_open_database_newer_than_binary() {
    // Blocker #53.8: an older binary opening a DB written by a newer
    // binary must refuse rather than run on a schema it does not
    // understand and silently corrupt data through positional column
    // access. We simulate the future DB by stamping a schema_version row
    // far ahead of any embedded migration.
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("baseline migrations failed");
    conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        [9999],
    )
    .expect("failed to stamp future schema version");

    let result = run_migrations(&conn);

    match result {
        Err(StorageError::SchemaTooNew {
            db_version,
            binary_version,
        }) => {
            assert_eq!(db_version, 9999);
            assert!(binary_version < db_version);
        }
        other => panic!("expected SchemaTooNew, got {:?}", other),
    }
}

#[test]
fn opens_database_at_exactly_the_binary_schema_version() {
    // The guard refuses only when the DB is strictly ahead; a DB at the
    // binary's own max version must open cleanly and idempotently.
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("first run failed");
    run_migrations(&conn).expect("equal-version reopen must succeed");
}
