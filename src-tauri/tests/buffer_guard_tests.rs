use tempfile::TempDir;
use writ_core::buffer::manager::BufferManager;
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_tauri_lib::commands::buffer::{decide_create_buffer, CreateDecision};

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    (dir, BufferStore::new(conn, buffers_dir))
}

/// Persists a fresh empty scratch buffer the way `create_buffer` does:
/// mint via the manager, insert, write empty content.
fn persist_empty_scratch(store: &BufferStore, mgr: &mut BufferManager) -> String {
    let doc = mgr.create_buffer(None).expect("mint");
    store.insert(&doc).expect("insert");
    store.save_content(&doc.id, "").expect("save empty");
    doc.id
}

#[test]
fn create_buffer_reuses_existing_empty_scratch() {
    let (_dir, store) = setup();
    let mut mgr = BufferManager::new();
    let existing_id = persist_empty_scratch(&store, &mut mgr);

    let decision = decide_create_buffer(&store, &mut mgr, None).expect("decide");
    match decision {
        CreateDecision::Reuse(doc) => assert_eq!(doc.id, existing_id),
        CreateDecision::Create(_) => panic!("expected reuse of empty scratch, got create"),
    }
}

#[test]
fn create_buffer_mints_new_when_no_empty_scratch() {
    let (_dir, store) = setup();
    let mut mgr = BufferManager::new();

    let decision = decide_create_buffer(&store, &mut mgr, None).expect("decide");
    assert!(matches!(decision, CreateDecision::Create(_)));
}

#[test]
fn create_buffer_with_explicit_title_always_mints() {
    let (_dir, store) = setup();
    let mut mgr = BufferManager::new();
    persist_empty_scratch(&store, &mut mgr);

    let decision =
        decide_create_buffer(&store, &mut mgr, Some("Named".to_string())).expect("decide");
    match decision {
        CreateDecision::Create(doc) => assert_eq!(doc.title, "Named"),
        CreateDecision::Reuse(_) => panic!("explicit title must never reuse"),
    }
}
