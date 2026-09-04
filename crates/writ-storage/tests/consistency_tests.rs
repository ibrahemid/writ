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
        read_only: false,
        size_bytes: 0,
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
fn anything_left_in_the_retired_folder_is_reported() {
    // After the notes migration the folder is empty. A file still in it is
    // text the migration could not place, kept rather than deleted.
    let (_dir, store) = setup_with_store();
    std::fs::write(store.buffers_dir().join("left_behind.txt"), "kept")
        .expect("failed to write the leftover");

    let report = ConsistencyChecker::new(&store)
        .check()
        .expect("check failed");

    assert_eq!(report.orphan_files, vec!["left_behind.txt".to_string()]);
}

#[test]
fn an_empty_retired_folder_reports_nothing() {
    let (_dir, store) = setup_with_store();

    let report = ConsistencyChecker::new(&store)
        .check()
        .expect("check failed");

    assert!(report.orphan_files.is_empty());
}

#[test]
fn a_note_with_no_file_is_reported_as_missing() {
    let (_dir, store) = setup_with_store();
    store
        .insert(&make_doc("missing-buf"))
        .expect("insert failed");

    let report = ConsistencyChecker::new(&store)
        .check()
        .expect("check failed");

    assert!(
        report.missing_files.contains(&"missing-buf".to_string()),
        "got: {:?}",
        report.missing_files
    );
}

#[test]
fn a_note_whose_file_vanished_is_reported_as_missing() {
    let (dir, store) = setup_with_store();
    let file = dir.path().join("gone.md");
    std::fs::write(&file, "for now").unwrap();
    let mut doc = make_doc("vanished-buf");
    doc.source_path = Some(file.to_string_lossy().into_owned());
    store.insert(&doc).expect("insert failed");

    assert!(ConsistencyChecker::new(&store)
        .check()
        .unwrap()
        .missing_files
        .is_empty());

    std::fs::remove_file(&file).unwrap();

    assert!(ConsistencyChecker::new(&store)
        .check()
        .unwrap()
        .missing_files
        .contains(&"vanished-buf".to_string()));
}
