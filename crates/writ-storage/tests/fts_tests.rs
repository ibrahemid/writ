use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::database::queries;
use writ_storage::fts::FtsIndex;

fn setup() -> (TempDir, rusqlite::Connection) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    (dir, conn)
}

fn insert_test_buffer(conn: &rusqlite::Connection, id: &str, title: &str) {
    let now = Utc::now();
    let doc = BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
        filename: format!("{}.md", id),
        status: BufferStatus::Active,
        language: None,
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
    };
    queries::insert_buffer(conn, &doc).unwrap();
}

#[test]
fn insert_and_search() {
    let (_dir, conn) = setup();
    let fts = FtsIndex::new(&conn);

    insert_test_buffer(&conn, "buf-a", "Algorithms");
    insert_test_buffer(&conn, "buf-b", "Sorting");

    fts.insert("buf-a", "Algorithms", "how to sort array efficiently")
        .expect("fts insert buf-a failed");
    fts.insert("buf-b", "Sorting", "introduction to databases")
        .expect("fts insert buf-b failed");

    let results = fts.search("array").expect("search failed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "buf-a");
}

#[test]
fn delete_removes_from_index() {
    let (_dir, conn) = setup();
    let fts = FtsIndex::new(&conn);

    insert_test_buffer(&conn, "buf-c", "Delete Me");
    fts.insert("buf-c", "Delete Me", "content to be removed")
        .expect("fts insert failed");

    fts.delete("buf-c").expect("fts delete failed");

    let results = fts.search("removed").expect("search failed");
    assert!(results.is_empty());
}

#[test]
fn update_replaces_content() {
    let (_dir, conn) = setup();
    let fts = FtsIndex::new(&conn);

    insert_test_buffer(&conn, "buf-d", "Update Test");
    fts.insert("buf-d", "Update Test", "old content here")
        .expect("fts insert failed");

    fts.update("buf-d", "Update Test", "new content about rust")
        .expect("fts update failed");

    let old_results = fts.search("old").expect("search old failed");
    assert!(old_results.is_empty());

    let new_results = fts.search("rust").expect("search rust failed");
    assert_eq!(new_results.len(), 1);
    assert_eq!(new_results[0], "buf-d");
}
