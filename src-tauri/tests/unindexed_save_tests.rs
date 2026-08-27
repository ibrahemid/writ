//! Storage contract behind `commands::buffer::save_buffer_content_unindexed`:
//! generated content lands on disk without entering the search index, while the
//! title row written at creation keeps the buffer findable by name.

use tempfile::TempDir;
use writ_core::buffer::manager::BufferManager;
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

const NOTICES_TITLE: &str = "Third-party licences";
const NOTICES_BODY: &str = "MIT License\n\nCopyright (c) 2019 Mads Marquart\n";

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    std::fs::create_dir_all(dir.path().join("notes")).expect("notes dir");
    (dir, BufferStore::new(conn, buffers_dir))
}

/// Creates a titled note the way `create_buffer` plus a first save does: mint,
/// insert, attach the file the text is going to live in.
fn persist_titled(dir: &TempDir, store: &BufferStore, title: &str) -> String {
    let mut mgr = BufferManager::new();
    let doc = mgr.create_buffer(Some(title.to_string())).expect("mint");
    store.insert(&doc).expect("insert");
    let file = dir.path().join("notes").join(format!("{title}.md"));
    store
        .attach_source_path(&doc.id, file.to_str().expect("path"))
        .expect("attach");
    doc.id
}

#[test]
fn unindexed_save_keeps_generated_content_out_of_search() {
    let (_dir, store) = setup();
    let id = persist_titled(&_dir, &store, NOTICES_TITLE);

    store
        .save_content_without_index(&id, NOTICES_BODY)
        .expect("unindexed save");

    assert_eq!(store.read_content(&id).expect("read back"), NOTICES_BODY);
    assert!(
        store.search("Marquart").expect("search").is_empty(),
        "licence text must not be searchable"
    );
}

#[test]
fn unindexed_save_leaves_the_buffer_findable_by_title() {
    let (_dir, store) = setup();
    let id = persist_titled(&_dir, &store, NOTICES_TITLE);

    store
        .save_content_without_index(&id, NOTICES_BODY)
        .expect("unindexed save");

    assert_eq!(store.search("licences").expect("search"), vec![id]);
}

#[test]
fn the_indexing_save_is_what_makes_content_searchable() {
    let (_dir, store) = setup();
    let id = persist_titled(&_dir, &store, NOTICES_TITLE);

    store.save_content(&id, NOTICES_BODY).expect("indexed save");

    assert_eq!(store.search("Marquart").expect("search"), vec![id]);
}

#[test]
fn unindexed_save_refuses_a_read_only_buffer() {
    let (_dir, store) = setup();
    let mut mgr = BufferManager::new();
    let mut doc = mgr.create_buffer(Some("Binary".to_string())).expect("mint");
    doc.read_only = true;
    store.insert(&doc).expect("insert");

    let err = store
        .save_content_without_index(&doc.id, NOTICES_BODY)
        .expect_err("read-only buffer must not be written");
    assert!(err.to_string().contains("read-only"), "got: {err}");
}
