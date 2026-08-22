use std::collections::HashMap;

use tempfile::TempDir;
use writ_core::maintenance::{needs_vacuum, VACUUM_MIN_FREE_PAGES};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::maintenance::checkpoint_truncate;

fn setup_store() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    (dir, BufferStore::new(conn, buffers_dir))
}

/// Fills the snapshot table the way an always-on heartbeat used to: many
/// oversized rows, all but the retained few of which are then pruned away,
/// leaving a file that is almost entirely freelist.
fn bloat_with_snapshots(dir: &TempDir, store: &BufferStore, rows: usize) {
    let conn = open_database(&dir.path().join("writ.db")).expect("open db");
    let payload = "x".repeat(256 * 1024);
    for i in 0..rows {
        conn.execute(
            "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
             VALUES (?1, 1, ?2, datetime('now'), 0)",
            rusqlite::params![format!("bloat-{i}"), payload],
        )
        .expect("insert");
    }
    // The prune (and the checkpoint that follows it) is what moves those rows
    // onto the freelist of the database file.
    store
        .write_session_snapshot(&HashMap::new(), false)
        .expect("snapshot write");
}

#[test]
fn fresh_database_is_not_vacuumed() {
    let (_dir, store) = setup_store();
    let outcome = store.run_maintenance().expect("maintenance");
    assert!(
        !outcome.vacuumed,
        "a database with nothing to reclaim must not be rewritten"
    );
}

#[test]
fn maintenance_reclaims_the_space_left_by_pruned_snapshots() {
    let (dir, store) = setup_store();
    bloat_with_snapshots(&dir, &store, 60);

    let before = store.database_stats().expect("stats before");
    assert!(
        before.freelist_count > VACUUM_MIN_FREE_PAGES,
        "test setup did not produce enough free pages: {before:?}"
    );
    assert!(
        needs_vacuum(before.page_count, before.freelist_count),
        "test setup did not produce a bloated database: {before:?}"
    );

    let outcome = store.run_maintenance().expect("maintenance");

    assert!(outcome.vacuumed, "a bloated database must be vacuumed");
    assert_eq!(
        outcome.after.freelist_count, 0,
        "vacuum must leave no free pages behind"
    );
    assert!(
        outcome.after.page_count < before.page_count / 2,
        "vacuum must shrink the file: {before:?} -> {:?}",
        outcome.after
    );
}

#[test]
fn snapshot_writes_leave_no_write_ahead_log_behind() {
    let (dir, store) = setup_store();
    bloat_with_snapshots(&dir, &store, 8);

    let wal_bytes = std::fs::metadata(dir.path().join("writ.db-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert_eq!(
        wal_bytes, 0,
        "each snapshot write checkpoints, so the log must not accumulate"
    );
}

#[test]
fn checkpoint_truncates_the_write_ahead_log() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    conn.execute(
        "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
         VALUES ('log-test', 1, ?1, datetime('now'), 0)",
        rusqlite::params!["x".repeat(256 * 1024)],
    )
    .expect("insert");
    assert!(
        std::fs::metadata(dir.path().join("writ.db-wal"))
            .map(|m| m.len())
            .unwrap_or(0)
            > 0,
        "the insert should have landed in the log"
    );

    assert!(
        checkpoint_truncate(&conn).expect("checkpoint"),
        "checkpoint reported busy"
    );

    let wal_bytes = std::fs::metadata(dir.path().join("writ.db-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert_eq!(
        wal_bytes, 0,
        "the log must be truncated, not just folded in"
    );
}
