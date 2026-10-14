use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
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

/// Fills the snapshot table the way 0.5's always-on heartbeat did: many
/// oversized rows, each a copy of the open notes. The first launch of 0.6
/// empties the text out of them and the next marker prunes all but the
/// retained few, leaving a file that is almost entirely freelist.
fn bloat_with_snapshots(dir: &TempDir, store: &BufferStore, rows: usize) {
    let conn = open_database(&dir.path().join("writ.db")).expect("open db");
    let payload = serde_json::json!({ "buffers": { "note": "x".repeat(256 * 1024) } }).to_string();
    for i in 0..rows {
        conn.execute(
            "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
             VALUES (?1, 1, ?2, datetime('now'), 0)",
            rusqlite::params![format!("bloat-{i}"), payload],
        )
        .expect("insert");
    }
    store
        .carry_over_legacy_snapshots()
        .expect("empty the text out of the snapshots");
    // The prune (and the checkpoint that follows it) is what moves those rows
    // onto the freelist of the database file.
    store.mark_session_clean().expect("marker write");
}

/// A note's row, which a journal entry needs to belong to.
fn note_row(id: &str) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: id.to_string(),
        filename: format!("{id}.txt"),
        status: BufferStatus::Active,
        language: None,
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
        encoding: None,
    }
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

/// The heartbeat runs for as long as Writ is open. Sixty beats is two hours
/// of it, and after its first it must leave the database the size it found
/// it: the bloat this guards against was a snapshot row per beat, each one a
/// full copy of every open note.
#[test]
fn sixty_heartbeats_write_one_marker_and_do_not_grow_the_database() {
    let (dir, mut store) = setup_store();

    assert!(
        store.mark_session_running().expect("first beat"),
        "the first beat records the running session"
    );
    let baseline = store.database_stats().expect("stats after the first beat");

    for beat in 1..60 {
        assert!(
            !store.mark_session_running().expect("beat"),
            "beat {beat} wrote another marker"
        );
    }

    let conn = open_database(&dir.path().join("writ.db")).expect("open db");
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM session_snapshots", [], |row| {
            row.get(0)
        })
        .expect("count snapshots");
    assert_eq!(rows, 1, "sixty beats left more than one row");

    let after = writ_storage::maintenance::read_stats(&conn).expect("stats after sixty beats");
    assert_eq!(
        after.file_bytes(),
        baseline.file_bytes(),
        "sixty beats grew the database file"
    );
}

/// A note whose saves keep failing and landing cycles its journal entry:
/// written as one fails, cleared as the next lands. The pages the first entry
/// freed are what the next one takes, so sixty cycles of a large note leave
/// the database the size the first left it.
#[test]
fn sixty_record_and_clear_cycles_do_not_grow_the_database() {
    let (_dir, store) = setup_store();
    store.insert(&note_row("note-a")).expect("insert the note");
    let text = "y".repeat(256 * 1024);

    store
        .record_unsaved_text("note-a", &text, Utc::now())
        .expect("record");
    store.clear_unsaved_text("note-a").expect("clear");
    let baseline = store.database_stats().expect("stats after the first cycle");

    for cycle in 1..60 {
        let typed = format!("{cycle}{text}");
        store
            .record_unsaved_text("note-a", &typed, Utc::now())
            .expect("record");
        assert!(store.clear_unsaved_text("note-a").expect("clear"));
    }

    let after = store.database_stats().expect("stats after sixty cycles");
    assert!(
        after.file_bytes() <= baseline.file_bytes() + 4 * after.page_size,
        "sixty cycles grew the database file: {baseline:?} -> {after:?}"
    );
    assert_eq!(store.unsaved_text_count().expect("count"), 0);
}
