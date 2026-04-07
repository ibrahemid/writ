use tempfile::TempDir;
use writ_core::buffer::document::BufferStatus;
use writ_core::buffer::manager::BufferManager;
use writ_core::config::WritConfig;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::recovery::snapshot::SnapshotManager;

#[test]
fn full_buffer_lifecycle() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("writ.db");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).unwrap();

    let conn = open_database(&db_path).unwrap();
    run_migrations(&conn).unwrap();

    let store = BufferStore::new(conn, buffers_dir);

    let mut mgr = BufferManager::new();
    let buf = mgr.create_buffer(Some("integration-test".into())).unwrap();
    store.insert(&buf).unwrap();

    store
        .save_content(&buf.id, "Hello from integration test")
        .unwrap();
    let content = store.read_content(&buf.id).unwrap();
    assert_eq!(content, "Hello from integration test");

    store.save_content(&buf.id, "Updated content").unwrap();
    let updated = store.read_content(&buf.id).unwrap();
    assert_eq!(updated, "Updated content");

    store.close(&buf.id).unwrap();
    let closed = store.get(&buf.id).unwrap();
    assert_eq!(closed.status, BufferStatus::History);

    let history = store.list_by_status(BufferStatus::History).unwrap();
    assert_eq!(history.len(), 1);

    store.restore(&buf.id).unwrap();
    let restored = store.get(&buf.id).unwrap();
    assert_eq!(restored.status, BufferStatus::Active);

    store.delete(&buf.id).unwrap();
    assert!(store.get(&buf.id).is_err());
}

#[test]
fn config_and_snapshot_lifecycle() {
    let dir = TempDir::new().unwrap();

    let config_path = dir.path().join("config.toml");
    let config_store = ConfigStore::new(config_path);
    let mut config = WritConfig::default();
    config.editor.font_size = 18;
    config_store.write(&config).unwrap();
    let loaded = config_store.read().unwrap();
    assert_eq!(loaded.editor.font_size, 18);

    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).unwrap();
    run_migrations(&conn).unwrap();
    let snapshot_mgr = SnapshotManager::new(&conn);

    let state = serde_json::json!({"active_tabs": ["buf1"]});
    snapshot_mgr.write_snapshot(&state, false).unwrap();
    let snapshot = snapshot_mgr.latest_snapshot().unwrap().unwrap();
    assert!(!snapshot.is_clean);

    snapshot_mgr.write_snapshot(&state, true).unwrap();
    let clean = snapshot_mgr.latest_snapshot().unwrap().unwrap();
    assert!(clean.is_clean);
}
