use std::collections::HashMap;

use tempfile::TempDir;
use writ_core::buffer::document::BufferStatus;
use writ_core::buffer::manager::BufferManager;
use writ_core::recovery::{resolve_recovery, RecoveryResolution};
use writ_storage::buffer_store::BufferStore;
use writ_storage::consistency::ConsistencyChecker;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::recovery::dirty_shutdown::check_dirty_shutdown;
use writ_storage::recovery::snapshot::SnapshotManager;

fn setup_store() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);
    (dir, store)
}

fn setup_conn() -> (TempDir, rusqlite::Connection) {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    (dir, conn)
}

// --- snapshot write / read / prune ---

#[test]
fn snapshot_write_and_read_roundtrip() {
    let (_dir, conn) = setup_conn();
    let mgr = SnapshotManager::new(&conn);
    let mut contents = HashMap::new();
    contents.insert("buf-1".to_string(), "hello world".to_string());
    let extra = serde_json::Value::Object(serde_json::Map::new());
    mgr.write_session_snapshot(&contents, &extra, false)
        .expect("write");
    let snap = mgr.latest_snapshot().expect("latest").expect("some");
    assert!(!snap.is_clean);
    let buffers = snap.state_json["buffers"]
        .as_object()
        .expect("buffers object");
    assert_eq!(buffers["buf-1"].as_str().expect("string"), "hello world");
}

#[test]
fn snapshot_clean_flag_roundtrip() {
    let (_dir, conn) = setup_conn();
    let mgr = SnapshotManager::new(&conn);
    let contents: HashMap<String, String> = HashMap::new();
    let extra = serde_json::Value::Object(serde_json::Map::new());
    mgr.write_session_snapshot(&contents, &extra, true)
        .expect("write");
    let snap = mgr.latest_snapshot().expect("latest").expect("some");
    assert!(snap.is_clean);
}

#[test]
fn snapshot_prune_keeps_bounded_count() {
    let (_dir, conn) = setup_conn();
    let mgr = SnapshotManager::new(&conn);
    let contents: HashMap<String, String> = HashMap::new();
    let extra = serde_json::Value::Object(serde_json::Map::new());

    // Write more than MAX_SNAPSHOTS (5)
    for _ in 0..8 {
        mgr.write_session_snapshot(&contents, &extra, false)
            .expect("write");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_snapshots", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(count, 5, "should have pruned to MAX_SNAPSHOTS");
}

// --- dirty-marker lifecycle ---

#[test]
fn no_snapshot_means_clean() {
    let (_dir, conn) = setup_conn();
    let is_dirty = check_dirty_shutdown(&conn).expect("check");
    assert!(!is_dirty, "no snapshot should not report dirty");
}

#[test]
fn dirty_snapshot_detected_as_dirty() {
    let (_dir, conn) = setup_conn();
    let mgr = SnapshotManager::new(&conn);
    let contents: HashMap<String, String> = HashMap::new();
    let extra = serde_json::Value::Object(serde_json::Map::new());
    mgr.write_session_snapshot(&contents, &extra, false)
        .expect("write");
    assert!(
        check_dirty_shutdown(&conn).expect("check"),
        "unclean snapshot should be dirty"
    );
}

#[test]
fn clean_snapshot_clears_dirty_flag() {
    let (_dir, conn) = setup_conn();
    let mgr = SnapshotManager::new(&conn);
    let contents: HashMap<String, String> = HashMap::new();
    let extra = serde_json::Value::Object(serde_json::Map::new());
    mgr.write_session_snapshot(&contents, &extra, false)
        .expect("dirty write");
    mgr.write_session_snapshot(&contents, &extra, true)
        .expect("clean write");
    assert!(
        !check_dirty_shutdown(&conn).expect("check"),
        "clean snapshot should clear dirty flag"
    );
}

// --- recovery resolution logic ---

#[test]
fn resolve_recovery_snapshot_newer_returns_restore() {
    let resolution = resolve_recovery("2024-01-01 12:00:01", "2024-01-01 12:00:00");
    assert_eq!(resolution, RecoveryResolution::Restore);
}

#[test]
fn resolve_recovery_snapshot_older_returns_ignore() {
    let resolution = resolve_recovery("2024-01-01 11:59:59", "2024-01-01 12:00:00");
    assert_eq!(resolution, RecoveryResolution::Ignore);
}

#[test]
fn resolve_recovery_same_timestamp_returns_ignore() {
    let resolution = resolve_recovery("2024-01-01 12:00:00", "2024-01-01 12:00:00");
    assert_eq!(resolution, RecoveryResolution::Ignore);
}

// --- recover_buffers integration ---

#[test]
fn recover_buffers_no_snapshot_returns_empty() {
    let (_dir, store) = setup_store();
    let recovered = store.resolve_recovery().expect("recover");
    assert!(recovered.is_empty());
}

#[test]
fn recover_buffers_clean_snapshot_returns_empty() {
    let (_dir, store) = setup_store();
    let mut contents = HashMap::new();
    contents.insert("buf-1".to_string(), "some content".to_string());
    store
        .write_session_snapshot(&contents, true)
        .expect("write clean");
    let recovered = store.resolve_recovery().expect("recover");
    assert!(
        recovered.is_empty(),
        "clean snapshot should not trigger recovery"
    );
}

/// Boot-path contract (#71): `AppState::initialize` runs dirty-shutdown
/// detection, recovery resolution, and the consistency check against the
/// SAME store instance, in that order. This is only possible because
/// `ConsistencyChecker` borrows the store rather than consuming it; a
/// regression to owning would break this sequence at compile time. The test
/// stands in for the unwired contract test deleted in this change.
#[test]
fn boot_sequence_runs_recovery_then_consistency_on_one_store() {
    let (_dir, store) = setup_store();
    let mut contents = HashMap::new();
    contents.insert("buf-crash".to_string(), "unsaved work".to_string());
    store
        .write_session_snapshot(&contents, false)
        .expect("write unclean snapshot");

    // 1. detection
    assert!(
        store.is_dirty_shutdown().expect("dirty check"),
        "an unclean snapshot must read back as a dirty shutdown"
    );
    // 2. recovery resolution (same store)
    let _ = store.resolve_recovery().expect("resolve recovery");
    // 3. consistency check (same store, borrowed)
    let report = ConsistencyChecker::new(&store)
        .check()
        .expect("consistency check");
    assert!(
        report.missing_files.is_empty(),
        "freshly written buffers must not report as missing: {:?}",
        report.missing_files
    );
}

// --- integration: simulate unclean shutdown ---

#[test]
fn unclean_shutdown_recovers_newer_snapshot_content() {
    let (_dir, store) = setup_store();
    let mut mgr = BufferManager::new();
    let doc = mgr
        .create_buffer(Some("test-recovery".into()))
        .expect("create");
    store.insert(&doc).expect("insert");
    store
        .save_content(&doc.id, "version at save time")
        .expect("save");

    // Simulate: snapshot was taken after the last DB write
    // We need the snapshot timestamp to be strictly newer.
    // SQLite datetime('now') has second granularity; sleep 1s is unreliable in CI.
    // Instead, craft the comparison via BufferStore::resolve_recovery which uses
    // updated_at from the DB vs snapshot.created_at. We manipulate the buffer's
    // updated_at to be one second in the past by inserting a custom doc.
    let past_updated_at = {
        let doc2 = store.list_by_status(BufferStatus::Active).expect("list")[0].clone();
        // Format updated_at as it comes from the DB query (Y-m-d H:M:S)
        doc2.updated_at.format("%Y-%m-%d %H:%M:%S").to_string()
    };

    // Write an unclean snapshot with content that is "newer"
    // by inserting a snapshot row with a future timestamp directly.
    // We do this via SnapshotManager using write_snapshot with a crafted state.
    {
        let dir_path = _dir.path().join("writ.db");
        let conn2 = open_database(&dir_path).expect("open second conn");
        run_migrations(&conn2).expect("migrations");
        let snap_state = serde_json::json!({
            "buffers": { &doc.id: "recovered content from crash" }
        });
        // Insert with a timestamp one second after updated_at
        let future_ts = chrono::DateTime::parse_from_str(
            &format!("{} +0000", past_updated_at),
            "%Y-%m-%d %H:%M:%S %z",
        )
        .expect("parse")
            + chrono::Duration::seconds(2);
        let ts_str = future_ts.format("%Y-%m-%d %H:%M:%S").to_string();
        conn2.execute(
            "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
             VALUES (?, 1, ?, ?, 0)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                serde_json::to_string(&snap_state).expect("serialize"),
                ts_str,
            ],
        ).expect("insert snapshot");
    }

    // Re-open store to get fresh connection
    let db_path = _dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = _dir.path().join("buffers");
    let store2 = BufferStore::new(conn, buffers_dir);

    assert!(store2.is_dirty_shutdown().expect("dirty check"));

    let recovered = store2.resolve_recovery().expect("recover");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, doc.id);
    assert_eq!(recovered[0].content, "recovered content from crash");
}

#[test]
fn collect_buffer_contents_reads_active_files() {
    let (_dir, store) = setup_store();
    let mut mgr = BufferManager::new();
    let doc = mgr
        .create_buffer(Some("collect-test".into()))
        .expect("create");
    store.insert(&doc).expect("insert");
    store
        .save_content(&doc.id, "buffer text content")
        .expect("save");

    let contents = store.collect_buffer_contents().expect("collect");
    assert_eq!(
        contents.get(&doc.id).map(String::as_str),
        Some("buffer text content")
    );
}
