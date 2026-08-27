use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::schema_meta;

fn setup() -> (TempDir, rusqlite::Connection) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    (dir, conn)
}

fn updated_at(conn: &rusqlite::Connection, key: &str) -> String {
    conn.query_row(
        "SELECT updated_at FROM schema_meta WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .expect("failed to read the stamp")
}

#[test]
fn set_then_get_round_trips() {
    let (_dir, conn) = setup();

    schema_meta::set(&conn, schema_meta::KEY_NOTES_MIGRATION_RAN_AT, "2026-08-28")
        .expect("set failed");

    let value =
        schema_meta::get(&conn, schema_meta::KEY_NOTES_MIGRATION_RAN_AT).expect("get failed");
    assert_eq!(value.as_deref(), Some("2026-08-28"));
    assert_eq!(
        schema_meta::get(&conn, "no-such-key").expect("get failed"),
        None
    );
}

#[test]
fn set_overwrites_and_restamps() {
    let (_dir, conn) = setup();
    let key = schema_meta::KEY_NOTES_MIGRATION_REPORT;

    schema_meta::set(&conn, key, "first").expect("first set failed");
    conn.execute(
        "UPDATE schema_meta SET updated_at = '2000-01-01T00:00:00+00:00' WHERE key = ?1",
        [key],
    )
    .expect("failed to age the stamp");

    schema_meta::set(&conn, key, "second").expect("second set failed");

    assert_eq!(
        schema_meta::get(&conn, key).expect("get failed").as_deref(),
        Some("second")
    );
    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .expect("failed to count rows");
    assert_eq!(rows, 1);

    let stamp = updated_at(&conn, key);
    assert_ne!(stamp, "2000-01-01T00:00:00+00:00");
    chrono::DateTime::parse_from_rfc3339(&stamp).expect("the stamp is not an RFC 3339 timestamp");
}

#[test]
fn clear_removes_the_row() {
    let (_dir, conn) = setup();
    let key = schema_meta::KEY_ROLLBACK_COPY_PATH;

    schema_meta::set(&conn, key, "/tmp/writ.db.pre-notes-migration").expect("set failed");
    schema_meta::clear(&conn, key).expect("clear failed");

    assert_eq!(schema_meta::get(&conn, key).expect("get failed"), None);
    schema_meta::clear(&conn, key).expect("clearing a missing row must succeed");
}
