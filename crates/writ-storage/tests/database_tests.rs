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
fn open_database_caps_the_write_ahead_log() {
    let (_dir, conn) = setup_temp_db();
    let limit: i64 = conn
        .query_row("PRAGMA journal_size_limit", [], |row| row.get(0))
        .expect("failed to query journal_size_limit");
    assert_eq!(limit, 64 * 1024 * 1024);
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

    // The index is keyed by path now (ADR-028 section 7): 040 creates
    // files_fts and 041 drops the row-keyed buffer_fts it replaces.
    let fts_tables: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type='table' AND name IN ('buffer_fts', 'files_fts')",
            )
            .expect("failed to prepare statement");
        stmt.query_map([], |row| row.get(0))
            .expect("query failed")
            .map(|r| r.expect("row error"))
            .collect()
    };

    assert!(fts_tables.contains(&"files_fts".to_string()));
    assert!(!fts_tables.contains(&"buffer_fts".to_string()));
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

#[test]
fn migration_040_adds_migrated_columns() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    let columns = buffer_columns(&conn);

    assert!(columns.contains(&"migrated_path".to_string()));
    assert!(columns.contains(&"migrated_at".to_string()));
}

#[test]
fn migration_040_creates_index_tables_empty() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    for table in [
        "schema_meta",
        "files",
        "links",
        "properties",
        "tags",
        "headings",
        "files_fts",
    ] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|err| panic!("{table} is not queryable: {err}"));
        assert_eq!(count, 0, "{table} should be created empty");
    }
}

#[test]
fn migration_040_files_fts_keeps_the_tokenizer_buffer_fts_had() {
    // buffer_fts is gone by 041, so the tokenizer and prefix set it carried
    // after migration 030 are asserted literally here: search behaviour must
    // not change when the index is re-keyed to paths.
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    let files_sql = table_sql(&conn, "files_fts");

    assert_eq!(
        quoted_option(&files_sql, "tokenize").as_deref(),
        Some("unicode61 remove_diacritics 2")
    );
    assert_eq!(
        quoted_option(&files_sql, "prefix").as_deref(),
        Some("2 3 4")
    );
}

#[test]
fn the_schema_migrations_are_idempotent_on_a_second_run() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");

    {
        let conn = open_database(&db_path).expect("failed to open database");
        run_migrations(&conn).expect("first run failed");
    }

    let conn = open_database(&db_path).expect("failed to reopen database");
    run_migrations(&conn).expect("second run failed");

    let applications: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_version WHERE version = 40",
            [],
            |row| row.get(0),
        )
        .expect("failed to count applications");
    assert_eq!(applications, 1);

    let max_version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .expect("failed to read max version");
    assert_eq!(max_version, 42);
}

#[test]
fn migration_042_records_how_a_file_was_indexed() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    let columns: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT name FROM pragma_table_info('files')")
            .expect("failed to prepare statement");
        stmt.query_map([], |row| row.get(0))
            .expect("query failed")
            .map(|r| r.expect("row error"))
            .collect()
    };
    assert!(columns.contains(&"indexed_by".to_string()));

    // A row written by anything that predates the column reads as content,
    // which is what every row in an existing database was.
    conn.execute(
        "INSERT INTO files (path, size, mtime, hash, indexed_at)
         VALUES ('/notes/old.md', 12, 1, 'abc', '2026-08-28T00:00:00Z')",
        [],
    )
    .expect("failed to insert file row");

    let indexed_by: String = conn
        .query_row(
            "SELECT indexed_by FROM files WHERE path = '/notes/old.md'",
            [],
            |row| row.get(0),
        )
        .expect("failed to read indexed_by");
    assert_eq!(indexed_by, "content");
}

#[test]
fn a_database_at_041_gains_the_column_with_its_rows_intact() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    // Roll the database back to the shape the release that ships 040 and 041
    // leaves behind, then migrate it forward again.
    conn.execute("ALTER TABLE files DROP COLUMN indexed_by", [])
        .expect("failed to drop the column");
    conn.execute("DELETE FROM schema_version WHERE version = 42", [])
        .expect("failed to roll the stamp back");
    conn.execute(
        "INSERT INTO files (path, size, mtime, hash, indexed_at)
         VALUES ('/notes/kept.md', 12, 1, 'abc', '2026-08-28T00:00:00Z')",
        [],
    )
    .expect("failed to insert file row");

    run_migrations(&conn).expect("migration to 042 failed");

    let (path, indexed_by): (String, String) = conn
        .query_row("SELECT path, indexed_by FROM files", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("failed to read the row back");
    assert_eq!(path, "/notes/kept.md");
    assert_eq!(
        indexed_by, "content",
        "an existing row was read to index it"
    );
}

#[test]
fn deleting_a_files_row_cascades_to_links_properties_tags_headings() {
    let (_dir, conn) = setup_temp_db();
    run_migrations(&conn).expect("migrations failed");

    conn.execute(
        "INSERT INTO files (path, size, mtime, hash, indexed_at)
         VALUES ('/notes/one.md', 12, 1, 'abc', '2026-08-28T00:00:00Z')",
        [],
    )
    .expect("failed to insert file row");
    conn.execute(
        "INSERT INTO links (from_path, to_target, to_path, kind, line, col)
         VALUES ('/notes/one.md', 'two', '/notes/two.md', 'wiki', 1, 0)",
        [],
    )
    .expect("failed to insert link row");
    conn.execute(
        "INSERT INTO properties (path, key, value_json)
         VALUES ('/notes/one.md', 'title', '\"One\"')",
        [],
    )
    .expect("failed to insert property row");
    conn.execute(
        "INSERT INTO tags (path, tag, line) VALUES ('/notes/one.md', 'daily', 3)",
        [],
    )
    .expect("failed to insert tag row");
    conn.execute(
        "INSERT INTO headings (path, level, text, line, slug)
         VALUES ('/notes/one.md', 1, 'One', 0, 'one')",
        [],
    )
    .expect("failed to insert heading row");

    conn.execute("DELETE FROM files WHERE path = '/notes/one.md'", [])
        .expect("failed to delete file row");

    for table in ["links", "properties", "tags", "headings"] {
        let remaining: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("failed to count rows");
        assert_eq!(remaining, 0, "{table} rows should cascade with the file");
    }
}

#[test]
fn a_0_3_5_database_migrates_to_040_with_its_rows_intact() {
    let (_dir, conn) = setup_temp_db();
    apply_0_3_5_schema(&conn);

    conn.execute(
        "INSERT INTO buffers (id, title, filename, status, created_at, updated_at)
         VALUES ('a', 'First', 'a.md', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .expect("failed to insert row");
    conn.execute(
        "INSERT INTO buffers (id, title, filename, status, created_at, updated_at)
         VALUES ('b', 'Second', 'b.md', 'history', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
        [],
    )
    .expect("failed to insert row");
    conn.execute(
        "INSERT INTO buffer_fts (rowid, title, content)
         SELECT rowid, title, 'hello' FROM buffers",
        [],
    )
    .expect("failed to index rows");

    run_migrations(&conn).expect("migration to 040 failed");

    let titles: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT title FROM buffers ORDER BY id")
            .expect("failed to prepare statement");
        stmt.query_map([], |row| row.get(0))
            .expect("query failed")
            .map(|r| r.expect("row error"))
            .collect()
    };
    assert_eq!(titles, vec!["First".to_string(), "Second".to_string()]);

    let indexed: i64 = conn
        .query_row("SELECT COUNT(*) FROM files_fts", [], |row| row.get(0))
        .expect("failed to count indexed rows");
    assert_eq!(
        indexed, 0,
        "the path-keyed index starts empty; the reconcile walk fills it"
    );

    let migrated_paths: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM buffers WHERE migrated_path IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("failed to count migrated rows");
    assert_eq!(migrated_paths, 0, "040 moves no data");

    let max_version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .expect("failed to read max version");
    assert_eq!(max_version, 42);
}

/// Column names of the `buffers` table.
fn buffer_columns(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('buffers')")
        .expect("failed to prepare statement");
    let columns = stmt
        .query_map([], |row| row.get(0))
        .expect("query failed")
        .map(|r| r.expect("row error"))
        .collect();
    columns
}

/// The `CREATE` statement SQLite recorded for a table.
fn table_sql(conn: &rusqlite::Connection, table: &str) -> String {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE name = ?1",
        [table],
        |row| row.get(0),
    )
    .unwrap_or_else(|err| panic!("no recorded statement for {table}: {err}"))
}

/// The value of a single-quoted FTS5 option, e.g. `tokenize='…'`.
fn quoted_option(sql: &str, option: &str) -> Option<String> {
    let needle = format!("{option}=");
    let after = sql.find(&needle)? + needle.len();
    let rest = &sql[after..];
    let rest = rest.strip_prefix('\'')?;
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// The schema a 0.3.5 database carries: migrations 001 through 030 and the
/// version rows that record them.
fn apply_0_3_5_schema(conn: &rusqlite::Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )
    .expect("failed to create the version table");

    const PRIOR: &[(i32, &str)] = &[
        (1, include_str!("../migrations/001_initial.sql")),
        (10, include_str!("../migrations/010_layout_state.sql")),
        (20, include_str!("../migrations/020_buffer_open_mode.sql")),
        (30, include_str!("../migrations/030_fts_prefix.sql")),
    ];
    for (version, sql) in PRIOR {
        conn.execute_batch(sql)
            .unwrap_or_else(|err| panic!("migration {version} failed: {err}"));
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
            [version],
        )
        .expect("failed to stamp version");
    }
}
