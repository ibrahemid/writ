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

fn make_source_doc(id: &str, title: &str, source_path: &str) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
        filename: format!("{}-{}", id, title),
        status: BufferStatus::Active,
        language: Some("rust".to_string()),
        source_path: Some(source_path.to_string()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
    }
}

fn make_scratch_doc(id: &str, title: &str) -> BufferDocument {
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
fn open_from_path_creates_buffer_and_copies_content() {
    let (dir, store) = setup();
    let source_dir = TempDir::new().unwrap();
    let source_file = source_dir.path().join("main.rs");
    std::fs::write(&source_file, "fn main() {}").unwrap();

    let doc = make_source_doc("open-1", "main.rs", source_file.to_str().unwrap());
    store.open_from_path(&doc, "fn main() {}").unwrap();

    let fetched = store.get("open-1").unwrap();
    assert_eq!(fetched.title, "main.rs");
    assert_eq!(
        fetched.source_path.as_deref(),
        Some(source_file.to_str().unwrap())
    );
    assert_eq!(fetched.language.as_deref(), Some("rust"));

    let buffer_copy = dir.path().join("buffers").join(&doc.filename);
    assert!(buffer_copy.exists());
    let content = std::fs::read_to_string(&buffer_copy).unwrap();
    assert_eq!(content, "fn main() {}");
}

#[test]
fn open_from_path_indexes_content_for_fts() {
    let (_dir, store) = setup();
    let doc = make_source_doc("fts-open", "main.rs", "/fake/main.rs");
    store
        .open_from_path(&doc, "fn search_me_please() {}")
        .unwrap();

    let results = store.search("search_me_please").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "fts-open");
}

#[test]
fn find_active_by_source_path_returns_existing_buffer() {
    let (_dir, store) = setup();
    let doc = make_source_doc("dedup-1", "config.toml", "/home/user/config.toml");
    store.open_from_path(&doc, "key = \"value\"").unwrap();

    let found = store
        .find_active_by_source_path("/home/user/config.toml")
        .unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, "dedup-1");
}

#[test]
fn find_active_by_source_path_returns_none_when_not_found() {
    let (_dir, store) = setup();
    let found = store
        .find_active_by_source_path("/nonexistent/path.txt")
        .unwrap();
    assert!(found.is_none());
}

#[test]
fn find_active_by_source_path_ignores_history_buffers() {
    let (_dir, store) = setup();
    let doc = make_source_doc("hist-1", "old.rs", "/home/user/old.rs");
    store.open_from_path(&doc, "old content").unwrap();
    store.close("hist-1").unwrap();

    let found = store
        .find_active_by_source_path("/home/user/old.rs")
        .unwrap();
    assert!(found.is_none());
}

#[test]
fn save_to_source_writes_to_original_file_and_buffer_copy() {
    let (dir, store) = setup();
    let source_dir = TempDir::new().unwrap();
    let source_file = source_dir.path().join("notes.md");
    std::fs::write(&source_file, "# Old").unwrap();

    let doc = make_source_doc("save-1", "notes.md", source_file.to_str().unwrap());
    store.open_from_path(&doc, "# Old").unwrap();

    store.save_to_source("save-1", "# Updated").unwrap();

    let source_content = std::fs::read_to_string(&source_file).unwrap();
    assert_eq!(source_content, "# Updated");

    let buffer_copy = dir.path().join("buffers").join(&doc.filename);
    let buffer_content = std::fs::read_to_string(&buffer_copy).unwrap();
    assert_eq!(buffer_content, "# Updated");
}

#[test]
fn save_to_source_updates_fts_index() {
    let (_dir, store) = setup();
    let source_dir = TempDir::new().unwrap();
    let source_file = source_dir.path().join("search.txt");
    std::fs::write(&source_file, "old content").unwrap();

    let doc = make_source_doc("fts-save", "search.txt", source_file.to_str().unwrap());
    store.open_from_path(&doc, "old content").unwrap();

    store
        .save_to_source("fts-save", "new unique findable content")
        .unwrap();

    let results = store.search("findable").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "fts-save");

    let old_results = store.search("old content").unwrap();
    assert!(old_results.is_empty());
}

#[test]
fn save_to_source_fails_for_scratch_buffer() {
    let (_dir, store) = setup();
    let doc = make_scratch_doc("scratch-1", "notes");
    store.insert(&doc).unwrap();
    store.save_content("scratch-1", "scratch content").unwrap();

    let result = store.save_to_source("scratch-1", "content");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("no source_path"), "error: {}", err);
}

#[test]
fn read_content_works_for_source_backed_buffer() {
    let (_dir, store) = setup();
    let doc = make_source_doc("read-src", "file.rs", "/fake/file.rs");
    store.open_from_path(&doc, "fn hello() {}").unwrap();

    let content = store.read_content("read-src").unwrap();
    assert_eq!(content, "fn hello() {}");
}

#[test]
fn update_language_sets_language_field() {
    let (_dir, store) = setup();
    let doc = make_scratch_doc("lang-1", "test");
    store.insert(&doc).unwrap();

    store.update_language("lang-1", Some("python")).unwrap();
    let fetched = store.get("lang-1").unwrap();
    assert_eq!(fetched.language.as_deref(), Some("python"));
}

#[test]
fn update_language_clears_language_field() {
    let (_dir, store) = setup();
    let doc = make_source_doc("lang-2", "file.rs", "/fake/file.rs");
    store.open_from_path(&doc, "content").unwrap();

    store.update_language("lang-2", None).unwrap();
    let fetched = store.get("lang-2").unwrap();
    assert!(fetched.language.is_none());
}

#[test]
fn close_and_restore_source_backed_buffer() {
    let (_dir, store) = setup();
    let doc = make_source_doc("lifecycle-1", "app.ts", "/home/user/app.ts");
    store.open_from_path(&doc, "const x = 1;").unwrap();

    store.close("lifecycle-1").unwrap();
    let closed = store.get("lifecycle-1").unwrap();
    assert_eq!(closed.status, BufferStatus::History);

    store.restore("lifecycle-1").unwrap();
    let restored = store.get("lifecycle-1").unwrap();
    assert_eq!(restored.status, BufferStatus::Active);
    assert_eq!(restored.source_path.as_deref(), Some("/home/user/app.ts"));
}

#[test]
fn find_history_by_source_path_returns_closed_buffer() {
    let (_dir, store) = setup();
    let doc = make_source_doc("hist-find", "closed.rs", "/home/user/closed.rs");
    store.open_from_path(&doc, "fn main() {}").unwrap();
    store.close("hist-find").unwrap();

    let found = store
        .find_history_by_source_path("/home/user/closed.rs")
        .unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, "hist-find");
}

#[test]
fn find_history_by_source_path_returns_none_for_active() {
    let (_dir, store) = setup();
    let doc = make_source_doc("active-only", "active.rs", "/home/user/active.rs");
    store.open_from_path(&doc, "content").unwrap();

    let found = store
        .find_history_by_source_path("/home/user/active.rs")
        .unwrap();
    assert!(found.is_none());
}

#[test]
fn reopen_from_history_restores_and_updates_content() {
    let (_dir, store) = setup();
    let source_dir = TempDir::new().unwrap();
    let source_file = source_dir.path().join("reopen.md");
    std::fs::write(&source_file, "# Version 1").unwrap();

    let doc = make_source_doc("reopen-1", "reopen.md", source_file.to_str().unwrap());
    store.open_from_path(&doc, "# Version 1").unwrap();
    store.close("reopen-1").unwrap();

    std::fs::write(&source_file, "# Version 2").unwrap();

    let history_buf = store
        .find_history_by_source_path(source_file.to_str().unwrap())
        .unwrap()
        .unwrap();
    store.restore(&history_buf.id).unwrap();
    store.save_content(&history_buf.id, "# Version 2").unwrap();

    let restored = store.get("reopen-1").unwrap();
    assert_eq!(restored.status, BufferStatus::Active);

    let content = store.read_content("reopen-1").unwrap();
    assert_eq!(content, "# Version 2");
}

#[test]
fn find_history_by_source_path_returns_none_when_not_found() {
    let (_dir, store) = setup();
    let found = store
        .find_history_by_source_path("/nonexistent/path.txt")
        .unwrap();
    assert!(found.is_none());
}

#[test]
fn reopen_preserves_original_buffer_id() {
    let (_dir, store) = setup();
    let source_dir = TempDir::new().unwrap();
    let source_file = source_dir.path().join("preserve.rs");
    std::fs::write(&source_file, "fn original() {}").unwrap();

    let doc = make_source_doc("preserve-1", "preserve.rs", source_file.to_str().unwrap());
    store.open_from_path(&doc, "fn original() {}").unwrap();
    store.close("preserve-1").unwrap();

    let history = store
        .find_history_by_source_path(source_file.to_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(history.id, "preserve-1");

    store.restore(&history.id).unwrap();
    let restored = store.get("preserve-1").unwrap();
    assert_eq!(restored.status, BufferStatus::Active);
    assert_eq!(
        restored.source_path.as_deref(),
        Some(source_file.to_str().unwrap())
    );
}

#[test]
fn delete_source_backed_buffer_removes_buffer_copy() {
    let (dir, store) = setup();
    let doc = make_source_doc("del-src", "remove.md", "/fake/remove.md");
    store.open_from_path(&doc, "will be deleted").unwrap();

    let buffer_copy = dir.path().join("buffers").join(&doc.filename);
    assert!(buffer_copy.exists());

    store.delete("del-src").unwrap();
    assert!(!buffer_copy.exists());
    assert!(store.get("del-src").is_err());
}
