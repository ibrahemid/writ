use chrono::Utc;
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::consistency::ConsistencyChecker;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::recovery::dirty_shutdown::check_dirty_shutdown;
use writ_storage::recovery::snapshot::SnapshotManager;

fn setup_conn() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    (dir, conn)
}

fn setup_with_store() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("failed to create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);
    (dir, store)
}

fn make_doc(id: &str) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: format!("Buffer {}", id),
        filename: format!("{}.txt", id),
        status: BufferStatus::Active,
        language: None,
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
    }
}

#[test]
fn snapshot_write_and_read() {
    let (_dir, conn) = setup_conn();
    let manager = SnapshotManager::new(&conn);
    let state = json!({ "open_buffers": [] });
    manager
        .write_snapshot(&state, false)
        .expect("write_snapshot failed");
    let snapshot = manager.latest_snapshot().expect("latest_snapshot failed");
    let snapshot = snapshot.expect("expected Some snapshot");
    assert_eq!(snapshot.format_version, 1);
    assert!(!snapshot.is_clean);
}

#[test]
fn clean_shutdown_marks_snapshot_clean() {
    let (_dir, conn) = setup_conn();
    let manager = SnapshotManager::new(&conn);
    let state = json!({ "open_buffers": [] });
    manager
        .write_snapshot(&state, true)
        .expect("write_snapshot failed");
    let snapshot = manager.latest_snapshot().expect("latest_snapshot failed");
    let snapshot = snapshot.expect("expected Some snapshot");
    assert!(snapshot.is_clean);
}

#[test]
fn dirty_shutdown_detected() {
    let (_dir, conn) = setup_conn();
    let manager = SnapshotManager::new(&conn);
    let state = json!({ "open_buffers": ["buf-1"] });
    manager
        .write_snapshot(&state, false)
        .expect("write_snapshot failed");
    let is_dirty = check_dirty_shutdown(&conn).expect("check_dirty_shutdown failed");
    assert!(is_dirty);
}

#[test]
fn orphan_file_detected() {
    let (_dir, store) = setup_with_store();
    let orphan_path = store.buffers_dir().join("orphan_extra.txt");
    std::fs::write(&orphan_path, "orphan content").expect("failed to write orphan file");
    let checker = ConsistencyChecker::new(store);
    let report = checker.check().expect("check failed");
    assert!(
        report
            .orphan_files
            .contains(&"orphan_extra.txt".to_string()),
        "expected orphan_extra.txt in orphan_files, got: {:?}",
        report.orphan_files
    );
}

#[test]
fn missing_file_detected() {
    let (_dir, store) = setup_with_store();
    let doc = make_doc("missing-buf");
    store.insert(&doc).expect("insert failed");
    let checker = ConsistencyChecker::new(store);
    let report = checker.check().expect("check failed");
    assert!(
        report.missing_files.contains(&"missing-buf".to_string()),
        "expected missing-buf in missing_files, got: {:?}",
        report.missing_files
    );
}
