use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("failed to create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);
    (dir, store)
}

fn make_doc(id: &str, title: &str) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
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
fn insert_and_get_buffer() {
    let (_dir, store) = setup();
    let doc = make_doc("buf-1", "Hello World");
    store.insert(&doc).expect("insert failed");
    let fetched = store.get("buf-1").expect("get failed");
    assert_eq!(fetched.title, "Hello World");
    assert_eq!(fetched.id, "buf-1");
}

#[test]
fn save_content_writes_file() {
    let (_dir, store) = setup();
    let doc = make_doc("buf-2", "Content Test");
    store.insert(&doc).expect("insert failed");
    store
        .save_content("buf-2", "Hello, file content!")
        .expect("save_content failed");
    let content = store.read_content("buf-2").expect("read_content failed");
    assert_eq!(content, "Hello, file content!");
}

#[test]
fn update_status_to_history() {
    let (_dir, store) = setup();
    let doc = make_doc("buf-3", "Close Me");
    store.insert(&doc).expect("insert failed");
    store.close("buf-3").expect("close failed");
    let fetched = store.get("buf-3").expect("get failed");
    assert_eq!(fetched.status, BufferStatus::History);
    assert!(fetched.closed_at.is_some());
}

#[test]
fn delete_removes_row_and_file() {
    let (_dir, store) = setup();
    let doc = make_doc("buf-4", "Delete Me");
    store.insert(&doc).expect("insert failed");
    store
        .save_content("buf-4", "temporary content")
        .expect("save_content failed");
    store.delete("buf-4").expect("delete failed");
    let get_result = store.get("buf-4");
    assert!(get_result.is_err());
    let read_result = store.read_content("buf-4");
    assert!(read_result.is_err());
}

#[test]
fn list_by_status() {
    let (_dir, store) = setup();
    let doc_a = make_doc("buf-5a", "Active Buffer");
    let doc_b = make_doc("buf-5b", "History Buffer");
    store.insert(&doc_a).expect("insert doc_a failed");
    store.insert(&doc_b).expect("insert doc_b failed");
    store.close("buf-5b").expect("close doc_b failed");

    let active = store
        .list_by_status(BufferStatus::Active)
        .expect("list active failed");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "buf-5a");

    let history = store
        .list_by_status(BufferStatus::History)
        .expect("list history failed");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, "buf-5b");
}

#[test]
fn update_tab_order() {
    let (_dir, store) = setup();
    let doc = make_doc("buf-6", "Tab Order");
    store.insert(&doc).expect("insert failed");
    store
        .update_tab_order("buf-6", 5)
        .expect("update_tab_order failed");
    let fetched = store.get("buf-6").expect("get failed");
    assert_eq!(fetched.tab_order, 5);
}

#[test]
fn save_content_updates_fts_index() {
    let (_dir, store) = setup();
    let doc = make_doc("fts1", "fts-test");
    store.insert(&doc).unwrap();
    store
        .save_content("fts1", "searchable content about rust programming")
        .unwrap();

    let results = store.search("rust programming").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "fts1");
}

#[test]
fn delete_removes_buffer_from_fts_index() {
    let (_dir, store) = setup();
    let doc = make_doc("orphan-1", "untitled");
    store.insert(&doc).unwrap();
    store.save_content("orphan-1", "foobar baseline").unwrap();
    assert_eq!(store.search("foobar").unwrap(), vec!["orphan-1"]);

    store.delete("orphan-1").expect("delete failed");

    assert!(
        store.search("foobar").unwrap().is_empty(),
        "deleting a buffer must also drop its FTS row"
    );
}

#[test]
fn search_finds_only_live_buffers_after_reinsert() {
    let (_dir, store) = setup();

    let first = make_doc("recycle-a", "untitled");
    store.insert(&first).unwrap();
    store.save_content("recycle-a", "foobar in first buffer").unwrap();
    store.delete("recycle-a").unwrap();

    let second = make_doc("recycle-b", "untitled");
    store.insert(&second).unwrap();
    store.save_content("recycle-b", "foobar in second buffer").unwrap();

    let hits = store.search("foobar").unwrap();
    assert_eq!(hits, vec!["recycle-b"]);
}

#[test]
fn rebuild_fts_recovers_index_from_buffers() {
    let (_dir, store) = setup();
    let doc = make_doc("rebuild-1", "untitled");
    store.insert(&doc).unwrap();
    store.save_content("rebuild-1", "lorem ipsum dolor").unwrap();

    store
        .rebuild_fts()
        .expect("rebuild_fts must succeed on a healthy store");

    let hits = store.search("ipsum").unwrap();
    assert_eq!(hits, vec!["rebuild-1"]);
}

#[test]
fn rename_buffer_updates_title() {
    let (_dir, store) = setup();
    let doc = make_doc("ren1", "original-title");
    store.insert(&doc).unwrap();
    store.rename("ren1", "new-title").unwrap();
    let updated = store.get("ren1").unwrap();
    assert_eq!(updated.title, "new-title");
}

#[test]
fn rename_refreshes_fts_title_so_new_title_is_searchable() {
    let (_dir, store) = setup();
    let doc = make_doc("ren-fts-a", "alpha");
    store.insert(&doc).unwrap();
    store.save_content("ren-fts-a", "body text").unwrap();

    store.rename("ren-fts-a", "beta").unwrap();

    let hits = store.search("beta").unwrap();
    assert_eq!(hits, vec!["ren-fts-a"]);
}

#[test]
fn rename_removes_old_title_from_fts_search_results() {
    let (_dir, store) = setup();
    let doc = make_doc("ren-fts-b", "alpha");
    store.insert(&doc).unwrap();
    store.save_content("ren-fts-b", "body text").unwrap();
    assert_eq!(store.search("alpha").unwrap(), vec!["ren-fts-b"]);

    store.rename("ren-fts-b", "beta").unwrap();

    assert!(
        store.search("alpha").unwrap().is_empty(),
        "old title must not survive in the FTS index after rename"
    );
}

#[test]
fn rename_preserves_content_searchability() {
    let (_dir, store) = setup();
    let doc = make_doc("ren-fts-c", "alpha");
    store.insert(&doc).unwrap();
    store
        .save_content("ren-fts-c", "lorem ipsum dolor sit amet")
        .unwrap();

    store.rename("ren-fts-c", "beta").unwrap();

    let hits = store.search("ipsum").unwrap();
    assert_eq!(hits, vec!["ren-fts-c"]);
}
