//! Storage contract behind `commands::buffer::save_buffer_content_unindexed`:
//! generated content lands on disk without entering the search index, and the
//! deferred reindex (ADR-020) is what puts a note's text there afterwards.

use tempfile::TempDir;
use writ_core::buffer::manager::BufferManager;
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::NotesIndexStore;

const NOTICES_TITLE: &str = "Third-party licences";
const NOTICES_BODY: &str = "MIT License\n\nCopyright (c) 2019 Mads Marquart\n";

fn setup() -> (TempDir, BufferStore, NotesIndexStore) {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    let notes_dir = dir.path().join("notes");
    std::fs::create_dir_all(&notes_dir).expect("notes dir");
    let mut store = BufferStore::new(conn, buffers_dir);
    store.set_notes_root(notes_dir);
    let index = NotesIndexStore::open(&db_path).expect("index db");
    (dir, store, index)
}

/// Paths whose text matches `raw`, through the same query policy the sidebar
/// search box uses.
fn search(index: &NotesIndexStore, raw: &str) -> Vec<String> {
    let Some(query) = writ_core::search::to_prefix_match(raw) else {
        return Vec::new();
    };
    let terms = writ_core::search::search_terms(raw);
    index
        .search_hits(&query, &terms, 50)
        .expect("search")
        .into_iter()
        .filter_map(|hit| hit.path)
        .collect()
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
    let (dir, store, index) = setup();
    let id = persist_titled(&dir, &store, NOTICES_TITLE);

    store
        .save_content_without_index(&id, NOTICES_BODY)
        .expect("unindexed save");

    assert_eq!(store.read_content(&id).expect("read back"), NOTICES_BODY);
    assert!(
        search(&index, "Marquart").is_empty(),
        "licence text must not be searchable"
    );
}

#[test]
fn the_deferred_reindex_is_what_makes_an_unindexed_save_searchable() {
    let (dir, store, index) = setup();
    let id = persist_titled(&dir, &store, NOTICES_TITLE);

    store
        .save_content_without_index(&id, NOTICES_BODY)
        .expect("unindexed save");
    assert!(search(&index, "Marquart").is_empty());

    // ADR-020: the write is durable immediately and only search freshness
    // lags, until the coalesced reindex catches up.
    store.reindex_buffer(&id).expect("reindex");

    assert_eq!(search(&index, "Marquart").len(), 1);
}

#[test]
fn the_indexing_save_is_what_makes_content_searchable() {
    let (dir, store, index) = setup();
    let id = persist_titled(&dir, &store, NOTICES_TITLE);

    store.save_content(&id, NOTICES_BODY).expect("indexed save");

    assert_eq!(search(&index, "Marquart").len(), 1);
}

#[test]
fn a_saved_note_is_findable_by_its_file_name() {
    let (dir, store, index) = setup();
    let id = persist_titled(&dir, &store, NOTICES_TITLE);
    store.save_content(&id, NOTICES_BODY).expect("indexed save");

    let hits = index.search_names("licences", 10).expect("search names");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, format!("{NOTICES_TITLE}.md"));
}

#[test]
fn unindexed_save_refuses_a_read_only_buffer() {
    let (_dir, store, _index) = setup();
    let mut mgr = BufferManager::new();
    let mut doc = mgr.create_buffer(Some("Binary".to_string())).expect("mint");
    doc.read_only = true;
    store.insert(&doc).expect("insert");

    let err = store
        .save_content_without_index(&doc.id, NOTICES_BODY)
        .expect_err("read-only buffer must not be written");
    assert!(err.to_string().contains("read-only"), "got: {err}");
}
