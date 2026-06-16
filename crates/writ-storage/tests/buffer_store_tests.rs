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
        read_only: false,
        size_bytes: 0,
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
fn save_content_without_index_persists_bytes_but_skips_fts() {
    // ADR-020 deferred path: the write is durable immediately, but the FTS
    // index is not touched, so the term is not yet searchable.
    let (_dir, store) = setup();
    store.insert(&make_doc("deferred-1", "deferred")).unwrap();
    store
        .save_content_without_index("deferred-1", "uniqueterm pending reindex")
        .expect("save_content_without_index failed");

    assert_eq!(
        store.read_content("deferred-1").unwrap(),
        "uniqueterm pending reindex",
        "bytes must be on disk immediately",
    );
    assert!(
        store.search("uniqueterm").unwrap().is_empty(),
        "deferred write must not be searchable before reindex",
    );
}

#[test]
fn reindex_buffer_makes_a_deferred_write_searchable() {
    let (_dir, store) = setup();
    store.insert(&make_doc("deferred-2", "deferred")).unwrap();
    store
        .save_content_without_index("deferred-2", "alpha beta gamma")
        .unwrap();
    assert!(store.search("beta").unwrap().is_empty());

    store.reindex_buffer("deferred-2").expect("reindex failed");
    assert_eq!(store.search("beta").unwrap(), vec!["deferred-2".to_string()]);
}

#[test]
fn reindex_buffer_reflects_latest_disk_content_after_coalesced_writes() {
    // Two deferred writes then a single reindex (the coalescing case): the
    // index must reflect only the latest bytes, never a stale intermediate.
    let (_dir, store) = setup();
    store.insert(&make_doc("deferred-3", "deferred")).unwrap();
    store
        .save_content_without_index("deferred-3", "staleword first version")
        .unwrap();
    store
        .save_content_without_index("deferred-3", "freshword second version")
        .unwrap();

    store.reindex_buffer("deferred-3").expect("reindex failed");

    assert!(
        store.search("staleword").unwrap().is_empty(),
        "reindex must not surface a superseded intermediate",
    );
    assert_eq!(
        store.search("freshword").unwrap(),
        vec!["deferred-3".to_string()],
    );
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
fn close_many_closes_all_listed_active_buffers() {
    let (_dir, store) = setup();
    let a = make_doc("cm-a", "a");
    let b = make_doc("cm-b", "b");
    let c = make_doc("cm-c", "c");
    store.insert(&a).unwrap();
    store.insert(&b).unwrap();
    store.insert(&c).unwrap();

    store
        .close_many(&["cm-a".to_string(), "cm-c".to_string()])
        .unwrap();

    assert_eq!(store.get("cm-a").unwrap().status, BufferStatus::History);
    assert_eq!(store.get("cm-b").unwrap().status, BufferStatus::Active);
    assert_eq!(store.get("cm-c").unwrap().status, BufferStatus::History);
}

#[test]
fn close_many_skips_missing_ids_without_error() {
    let (_dir, store) = setup();
    let real = make_doc("cm-real", "real");
    store.insert(&real).unwrap();

    store
        .close_many(&[
            "cm-real".to_string(),
            "cm-ghost-1".to_string(),
            "cm-ghost-2".to_string(),
        ])
        .expect("missing ids must not error");

    assert_eq!(store.get("cm-real").unwrap().status, BufferStatus::History);
}

#[test]
fn close_many_is_noop_on_empty_input() {
    let (_dir, store) = setup();
    let a = make_doc("cm-empty-a", "a");
    store.insert(&a).unwrap();

    store.close_many(&[]).expect("empty close_many is a no-op");

    assert_eq!(
        store.get("cm-empty-a").unwrap().status,
        BufferStatus::Active
    );
}

#[test]
fn close_many_rolls_back_every_buffer_when_a_close_fails_mid_transaction() {
    let (dir, store) = setup();
    for id in ["cm-tx-keep", "cm-tx-trap"] {
        let doc = make_doc(id, id);
        store.insert(&doc).unwrap();
    }

    // Trap the second close: a trigger raises on any UPDATE to `cm-tx-trap`,
    // so the in-transaction `close_buffer` for it aborts. The whole
    // transaction must roll back, leaving the first buffer Active rather than
    // closing it before the second errors. The trigger is created on a second
    // connection (committed to the shared db file) so the store's connection
    // sees it.
    second_conn(&dir)
        .execute_batch(
            "CREATE TRIGGER cm_trap BEFORE UPDATE ON buffers \
             WHEN NEW.id = 'cm-tx-trap' \
             BEGIN SELECT RAISE(ABORT, 'trapped'); END;",
        )
        .unwrap();

    let result = store.close_many(&["cm-tx-keep".to_string(), "cm-tx-trap".to_string()]);

    assert!(result.is_err(), "a mid-transaction failure must propagate");
    assert_eq!(
        store.get("cm-tx-keep").unwrap().status,
        BufferStatus::Active,
        "a mid-transaction failure must roll back every close in the batch"
    );
}

#[test]
fn delete_many_removes_rows_files_and_fts() {
    let (dir, store) = setup();
    for id in ["dm-a", "dm-b", "dm-c"] {
        let doc = make_doc(id, id);
        store.insert(&doc).unwrap();
        store.save_content(id, "shared needle text").unwrap();
    }

    store
        .delete_many(&["dm-a".to_string(), "dm-c".to_string()])
        .expect("delete_many of valid ids must succeed");

    assert!(store.get("dm-a").is_err(), "dm-a row must be gone");
    assert!(store.get("dm-c").is_err(), "dm-c row must be gone");
    assert!(store.get("dm-b").is_ok(), "dm-b must be untouched");
    let buffers = dir.path().join("buffers");
    assert!(
        !buffers.join("dm-a.txt").exists() && !buffers.join("dm-c.txt").exists(),
        "backing files of deleted buffers must be removed after commit"
    );
    assert!(
        buffers.join("dm-b.txt").exists(),
        "the surviving buffer's backing file must remain"
    );
    assert_eq!(
        store.search("needle").unwrap(),
        vec!["dm-b"],
        "FTS rows of deleted buffers must be removed"
    );
}

#[test]
fn delete_many_rolls_back_every_row_when_a_delete_fails_mid_transaction() {
    let (dir, store) = setup();
    for id in ["dm-tx-1", "dm-tx-2"] {
        let doc = make_doc(id, id);
        store.insert(&doc).unwrap();
        store.save_content(id, "in transaction").unwrap();
    }

    // Drop the FTS table so the first in-transaction `fts.delete` fails. The
    // failure must roll the whole transaction back, leaving both rows intact
    // rather than deleting the first before the second errors.
    second_conn(&dir)
        .execute("DROP TABLE buffer_fts", [])
        .unwrap();

    let result = store.delete_many(&["dm-tx-1".to_string(), "dm-tx-2".to_string()]);

    assert!(result.is_err(), "a mid-transaction failure must propagate");
    assert!(
        store.get("dm-tx-1").is_ok() && store.get("dm-tx-2").is_ok(),
        "a mid-transaction failure must roll back every row in the batch"
    );
}

#[test]
fn delete_many_is_all_or_nothing_when_an_id_is_unknown() {
    let (_dir, store) = setup();
    for id in ["dm-keep-1", "dm-keep-2"] {
        let doc = make_doc(id, id);
        store.insert(&doc).unwrap();
        store.save_content(id, "persist me").unwrap();
    }

    let result = store.delete_many(&[
        "dm-keep-1".to_string(),
        "dm-ghost".to_string(),
        "dm-keep-2".to_string(),
    ]);

    assert!(result.is_err(), "an unknown id must abort the batch");
    assert!(
        store.get("dm-keep-1").is_ok(),
        "no buffer may be deleted when the batch aborts"
    );
    assert!(
        store.get("dm-keep-2").is_ok(),
        "no buffer may be deleted when the batch aborts"
    );
    assert_eq!(
        store.search("persist").unwrap().len(),
        2,
        "FTS must be untouched when the batch aborts"
    );
}

#[test]
fn delete_many_is_noop_on_empty_input() {
    let (_dir, store) = setup();
    let doc = make_doc("dm-solo", "solo");
    store.insert(&doc).unwrap();

    store.delete_many(&[]).expect("empty delete_many is a no-op");

    assert!(store.get("dm-solo").is_ok());
}

#[test]
fn rebuild_fts_indexes_both_active_and_history_buffers() {
    let (_dir, store) = setup();
    let active = make_doc("rf-active", "rf-active");
    let history = make_doc("rf-history", "rf-history");
    store.insert(&active).unwrap();
    store.insert(&history).unwrap();
    store.save_content("rf-active", "alpha sentinel").unwrap();
    store.save_content("rf-history", "beta sentinel").unwrap();
    store.close("rf-history").unwrap();

    store.rebuild_fts().expect("rebuild_fts must succeed");

    assert_eq!(store.search("alpha").unwrap(), vec!["rf-active"]);
    assert_eq!(
        store.search("beta").unwrap(),
        vec!["rf-history"],
        "history buffers must be reindexed, not just active ones"
    );
}

#[test]
fn rebuild_fts_tolerates_a_missing_backing_file() {
    let (_dir, store) = setup();
    let present = make_doc("rf-present", "rf-present");
    let orphan = make_doc("rf-orphan", "rf-orphan");
    store.insert(&present).unwrap();
    store.insert(&orphan).unwrap();
    store.save_content("rf-present", "findable body").unwrap();
    store.save_content("rf-orphan", "doomed body").unwrap();

    // Simulate a row whose content file vanished out from under the store.
    std::fs::remove_file(_dir.path().join("buffers").join("rf-orphan.txt")).unwrap();

    store
        .rebuild_fts()
        .expect("a missing backing file must not abort the rebuild");

    assert_eq!(store.search("findable").unwrap(), vec!["rf-present"]);
    assert!(
        store.search("doomed").unwrap().is_empty(),
        "the orphan's old body must not survive once its file is gone"
    );
}

#[test]
fn rebuild_fts_drops_stale_rows_for_deleted_buffers() {
    let (dir, store) = setup();
    let doc = make_doc("rf-stale", "rf-stale");
    store.insert(&doc).unwrap();
    store.save_content("rf-stale", "ghost token").unwrap();

    // Delete the buffer row directly, leaving its FTS row orphaned. The
    // orphan is invisible to `search` (which inner-joins on `buffers.rowid`),
    // so assert against the raw FTS row count instead.
    let probe = second_conn(&dir);
    probe
        .execute("DELETE FROM buffers WHERE id = 'rf-stale'", [])
        .unwrap();
    let stale: i64 = probe
        .query_row("SELECT count(*) FROM buffer_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stale, 1, "precondition: an orphan FTS row exists");

    store.rebuild_fts().expect("rebuild_fts must succeed");

    let after: i64 = probe
        .query_row("SELECT count(*) FROM buffer_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        after, 0,
        "rebuild_fts must purge FTS rows whose buffer no longer exists"
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

/// Builds a never-renamed scratch buffer: `title == filename`, active,
/// no `source_path`. Mirrors what `create_buffer` mints.
fn make_scratch(id: &str) -> BufferDocument {
    let mut doc = make_doc(id, "");
    doc.title = doc.filename.clone();
    doc
}

#[test]
fn find_empty_scratch_active_returns_none_on_empty_store() {
    let (_dir, store) = setup();
    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn find_empty_scratch_active_returns_zero_byte_unnamed_scratch() {
    let (_dir, store) = setup();
    let doc = make_scratch("scratch-1");
    store.insert(&doc).unwrap();
    store.save_content("scratch-1", "").unwrap();

    let found = store.find_empty_scratch_active().unwrap();
    assert_eq!(found.map(|d| d.id), Some("scratch-1".to_string()));
}

#[test]
fn find_empty_scratch_active_skips_buffer_with_content() {
    let (_dir, store) = setup();
    let doc = make_scratch("scratch-2");
    store.insert(&doc).unwrap();
    store.save_content("scratch-2", "hello").unwrap();

    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn find_empty_scratch_active_skips_renamed_scratch() {
    let (_dir, store) = setup();
    let mut doc = make_scratch("scratch-3");
    doc.title = "My Notes".to_string();
    store.insert(&doc).unwrap();
    store.save_content("scratch-3", "").unwrap();

    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn find_empty_scratch_active_skips_buffer_with_source_path() {
    let (_dir, store) = setup();
    let mut doc = make_scratch("scratch-4");
    doc.source_path = Some("/tmp/empty.txt".to_string());
    store.insert(&doc).unwrap();
    store.save_content("scratch-4", "").unwrap();

    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn find_empty_scratch_active_skips_history_buffer() {
    let (_dir, store) = setup();
    let doc = make_scratch("scratch-5");
    store.insert(&doc).unwrap();
    store.save_content("scratch-5", "").unwrap();
    store.close("scratch-5").unwrap();

    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn find_empty_scratch_active_returns_uuid_filename_scratch_with_default_writ_title() {
    let (_dir, store) = setup();
    let mut doc = make_doc("scratch-uuid-1", "writ-1700000000000");
    doc.filename = format!("{}.txt", doc.id);
    store.insert(&doc).unwrap();
    store.save_content("scratch-uuid-1", "").unwrap();

    let found = store.find_empty_scratch_active().unwrap();
    assert_eq!(found.map(|d| d.id), Some("scratch-uuid-1".to_string()));
}

#[test]
fn find_empty_scratch_active_skips_uuid_filename_buffer_with_custom_title() {
    let (_dir, store) = setup();
    let mut doc = make_doc("scratch-uuid-2", "My Notes");
    doc.filename = format!("{}.txt", doc.id);
    store.insert(&doc).unwrap();
    store.save_content("scratch-uuid-2", "").unwrap();

    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn reclaim_empty_scratch_deletes_empty_scratch_any_status_and_returns_count() {
    let (_dir, store) = setup();

    let active_empty = make_scratch("re-active-empty");
    store.insert(&active_empty).unwrap();
    store.save_content("re-active-empty", "").unwrap();

    let history_empty = make_scratch("re-history-empty");
    store.insert(&history_empty).unwrap();
    store.save_content("re-history-empty", "").unwrap();
    store.close("re-history-empty").unwrap();

    let with_content = make_scratch("re-content");
    store.insert(&with_content).unwrap();
    store.save_content("re-content", "keep me").unwrap();

    let count = store.reclaim_empty_scratch().unwrap();
    assert_eq!(count, 2);
    assert!(store.get("re-active-empty").is_err());
    assert!(store.get("re-history-empty").is_err());
    assert!(store.get("re-content").is_ok());
}

#[test]
fn reclaim_empty_scratch_keeps_named_nonempty_and_sourced_buffers() {
    let (_dir, store) = setup();

    let named = make_doc("kept-named", "Important");
    store.insert(&named).unwrap();
    store.save_content("kept-named", "").unwrap();

    let mut sourced = make_scratch("kept-sourced");
    sourced.source_path = Some("/tmp/real.txt".to_string());
    store.insert(&sourced).unwrap();
    store.save_content("kept-sourced", "").unwrap();

    let count = store.reclaim_empty_scratch().unwrap();
    assert_eq!(count, 0);
    assert!(store.get("kept-named").is_ok());
    assert!(store.get("kept-sourced").is_ok());
}

#[test]
fn reclaim_empty_scratch_removes_backing_files() {
    let (dir, store) = setup();
    let doc = make_scratch("re-file");
    store.insert(&doc).unwrap();
    store.save_content("re-file", "").unwrap();
    let file_path = dir.path().join("buffers").join(&doc.filename);
    assert!(file_path.exists());

    store.reclaim_empty_scratch().unwrap();

    assert!(!file_path.exists());
}

// Custom doc with a caller-chosen filename, to exercise legacy rows whose
// mirror filename predates the UUID-derived naming (audit blocker #53.7).
fn make_doc_with_filename(id: &str, title: &str, filename: &str) -> BufferDocument {
    let mut doc = make_doc(id, title);
    doc.title = title.to_string();
    doc.filename = filename.to_string();
    doc
}

#[test]
fn reconcile_renames_legacy_basename_filename_to_uuid() {
    let (dir, store) = setup();
    let buffers = dir.path().join("buffers");
    let doc = make_doc_with_filename("legacy-1", "notes.md", "notes.md");
    store.insert(&doc).unwrap();
    std::fs::write(buffers.join("notes.md"), "legacy content").unwrap();

    let count = store.reconcile_buffer_filenames().unwrap();
    assert_eq!(count, 1);

    let fetched = store.get("legacy-1").unwrap();
    assert_eq!(fetched.filename, "legacy-1.txt");
    assert_eq!(fetched.title, "notes.md");
    assert!(buffers.join("legacy-1.txt").exists());
    assert!(!buffers.join("notes.md").exists());
    assert_eq!(store.read_content("legacy-1").unwrap(), "legacy content");
}

#[test]
fn reconcile_is_idempotent() {
    let (dir, store) = setup();
    let buffers = dir.path().join("buffers");
    let doc = make_doc_with_filename("legacy-2", "todo.md", "todo.md");
    store.insert(&doc).unwrap();
    std::fs::write(buffers.join("todo.md"), "x").unwrap();

    assert_eq!(store.reconcile_buffer_filenames().unwrap(), 1);
    assert_eq!(store.reconcile_buffer_filenames().unwrap(), 0);
}

#[test]
fn reconcile_tolerates_missing_backing_file() {
    // The original collision left two rows pointing at one physical file;
    // after the first is renamed the second's source is already gone.
    // Reconciliation must still normalize the row, not panic.
    let (_dir, store) = setup();
    let doc = make_doc_with_filename("legacy-3", "gone.md", "gone.md");
    store.insert(&doc).unwrap();

    let count = store.reconcile_buffer_filenames().unwrap();
    assert_eq!(count, 1);
    assert_eq!(store.get("legacy-3").unwrap().filename, "legacy-3.txt");
}

#[test]
fn reconcile_establishes_unique_filename_index() {
    let (_dir, store) = setup();
    store.reconcile_buffer_filenames().unwrap();

    let a = make_doc_with_filename("uniq-a", "a", "dup.txt");
    let b = make_doc_with_filename("uniq-b", "b", "dup.txt");
    store.insert(&a).unwrap();
    assert!(
        store.insert(&b).is_err(),
        "the UNIQUE(filename) index must reject a duplicate mirror filename"
    );
}

// Opens a second connection to the same database file as `store`, used to
// corrupt the FTS index out from under the store and exercise the
// transactional-save and parity-repair paths (audit blocker #53.5).
fn second_conn(dir: &TempDir) -> rusqlite::Connection {
    open_database(&dir.path().join("test.db")).unwrap()
}

#[test]
fn save_content_propagates_fts_error_and_rolls_back_timestamp() {
    let (dir, store) = setup();
    let doc = make_doc("tx-1", "Title");
    store.insert(&doc).unwrap();
    store.save_content("tx-1", "first").unwrap();
    let before = store.get("tx-1").unwrap().updated_at;

    // Drop the FTS table so the next save's fts step fails; the timestamp
    // update must roll back with it, never leaving the row advanced while
    // the index is stale.
    second_conn(&dir)
        .execute("DROP TABLE buffer_fts", [])
        .unwrap();

    let result = store.save_content("tx-1", "second");
    assert!(result.is_err(), "a failing FTS update must propagate");

    let after = store.get("tx-1").unwrap().updated_at;
    assert_eq!(before, after, "timestamp must roll back when FTS fails");
}

#[test]
fn verify_and_repair_fts_rebuilds_after_drift() {
    let (dir, store) = setup();
    for (id, title) in [("p-1", "alpha"), ("p-2", "beta")] {
        let doc = make_doc(id, title);
        store.insert(&doc).unwrap();
        store.save_content(id, "needle haystack").unwrap();
    }
    // Corrupt the index: drop one buffer's FTS row directly.
    second_conn(&dir)
        .execute(
            "DELETE FROM buffer_fts WHERE rowid = (SELECT rowid FROM buffers WHERE id = 'p-1')",
            [],
        )
        .unwrap();
    assert!(!store.search("needle").unwrap().contains(&"p-1".to_string()));

    let repaired = store.verify_and_repair_fts().unwrap();
    assert!(repaired, "drift must trigger a rebuild");
    assert!(store.search("needle").unwrap().contains(&"p-1".to_string()));
}

#[test]
fn verify_and_repair_fts_is_noop_on_healthy_index() {
    let (_dir, store) = setup();
    let doc = make_doc("h-1", "Title");
    store.insert(&doc).unwrap();
    store.save_content("h-1", "content").unwrap();

    assert!(
        !store.verify_and_repair_fts().unwrap(),
        "a consistent index must not be rebuilt"
    );
}

#[test]
fn reconcile_resolves_two_rows_sharing_one_backing_file() {
    // The exact corruption the fix exists for: two legacy rows that minted
    // the same mirror filename and overwrote one physical file. Both must
    // end at distinct UUID-derived names with no panic and a successful
    // UNIQUE index build.
    let (dir, store) = setup();
    let buffers = dir.path().join("buffers");
    let a = make_doc_with_filename("collide-a", "notes.md", "notes.md");
    let b = make_doc_with_filename("collide-b", "notes.md", "notes.md");
    store.insert(&a).unwrap();
    store.insert(&b).unwrap();
    std::fs::write(buffers.join("notes.md"), "shared").unwrap();

    let count = store.reconcile_buffer_filenames().unwrap();
    assert_eq!(count, 2);

    assert_eq!(store.get("collide-a").unwrap().filename, "collide-a.txt");
    assert_eq!(store.get("collide-b").unwrap().filename, "collide-b.txt");
    // The surviving file went to whichever row reconciled first; the other
    // row is normalized but backing-file-less. Neither name collides now.
    let c = make_doc_with_filename("collide-c", "x", "collide-a.txt");
    assert!(
        store.insert(&c).is_err(),
        "UNIQUE(filename) must hold after a collision reconcile"
    );
}

#[test]
fn insert_seeds_fts_so_a_fresh_buffer_is_not_seen_as_drift() {
    // Audit blocker #53.5 hardening: an indexed-eligible buffer is in the
    // FTS index from insert, so the boot parity check never rebuilds for a
    // freshly inserted (never-content-saved) buffer.
    let (_dir, store) = setup();
    store.insert(&make_doc("seed-1", "Title")).unwrap();
    assert!(
        !store.verify_and_repair_fts().unwrap(),
        "a freshly inserted buffer must already be indexed"
    );
}
