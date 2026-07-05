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
        read_only: false,
        size_bytes: 0,
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
fn search_hits_return_line_number_and_highlighted_snippet() {
    let (_dir, conn) = setup();
    let fts = FtsIndex::new(&conn);

    insert_test_buffer(&conn, "buf-a", "notes.md");
    fts.insert(
        "buf-a",
        "notes.md",
        "first line\nthe rerank ceiling here\ntail",
    )
    .expect("fts insert failed");

    let terms = vec!["rerank".to_string()];
    let hits = fts
        .search_hits("\"rerank\"*", &terms, 50)
        .expect("search failed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].buffer_id, "buf-a");
    assert_eq!(hits[0].title, "notes.md");
    assert_eq!(hits[0].line, Some(2));
    let snippet: String = hits[0].snippet.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(snippet, "the rerank ceiling here");
    assert!(hits[0]
        .snippet
        .iter()
        .any(|s| s.matched && s.text == "rerank"));
}

#[test]
fn count_reports_total_matches_independent_of_limit() {
    let (_dir, conn) = setup();
    let fts = FtsIndex::new(&conn);

    for i in 0..5 {
        let id = format!("buf-{i}");
        insert_test_buffer(&conn, &id, "doc");
        fts.insert(&id, "doc", "shared keyword body")
            .expect("insert failed");
    }

    let terms = vec!["keyword".to_string()];
    let hits = fts
        .search_hits("\"keyword\"*", &terms, 2)
        .expect("search failed");
    assert_eq!(hits.len(), 2, "limit caps returned hits");
    assert_eq!(fts.count("\"keyword\"*").expect("count failed"), 5);
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
fn migrations_are_idempotent_after_fts_rebuild() {
    // The v30 FTS rebuild creates/drops/renames tables. Re-running migrations
    // on an already-migrated database must be a clean no-op, never failing on
    // an already-existing object — the symptom a non-atomic migration would
    // leave behind on a partial-failure retry.
    let (_dir, conn) = setup();
    run_migrations(&conn).expect("second migration run must be a no-op");
    run_migrations(&conn).expect("third migration run must be a no-op");

    // The prefix index still functions after repeated runs.
    let fts = FtsIndex::new(&conn);
    insert_test_buffer(&conn, "idem", "Tokenizer");
    fts.insert("idem", "Tokenizer", "token streams")
        .expect("fts insert failed");
    assert_eq!(
        fts.search("\"tok\"*").expect("prefix search failed"),
        vec!["idem".to_string()],
    );
}

#[test]
fn prefix_query_matches_longer_tokens() {
    // The prefix index (migration 030) is what makes search-as-you-type work:
    // a 3-character prefix term must hit longer tokens that share the prefix.
    let (_dir, conn) = setup();
    let fts = FtsIndex::new(&conn);

    insert_test_buffer(&conn, "buf-tok", "Tokenizer notes");
    insert_test_buffer(&conn, "buf-other", "Unrelated");
    fts.insert(
        "buf-tok",
        "Tokenizer notes",
        "the token stream is tokenized",
    )
    .expect("fts insert failed");
    fts.insert("buf-other", "Unrelated", "nothing in common")
        .expect("fts insert failed");

    let results = fts.search("\"tok\"*").expect("prefix search failed");
    assert_eq!(results, vec!["buf-tok".to_string()]);
}

#[test]
fn diacritics_are_folded_for_search() {
    // The unicode61 remove_diacritics=2 tokenizer (migration 030) folds
    // accents, so an ASCII query finds an accented term and vice versa.
    let (_dir, conn) = setup();
    let fts = FtsIndex::new(&conn);

    insert_test_buffer(&conn, "buf-cafe", "Café notes");
    fts.insert("buf-cafe", "Café notes", "résumé of the meeting")
        .expect("fts insert failed");

    assert_eq!(
        fts.search("resume").expect("search failed"),
        vec!["buf-cafe".to_string()],
    );
    assert_eq!(
        fts.search("cafe").expect("search failed"),
        vec!["buf-cafe".to_string()],
    );
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
