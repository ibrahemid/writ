use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::NotesIndexStore;

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("failed to create buffers dir");
    let mut store = BufferStore::new(conn, buffers_dir);
    store.set_notes_root(notes_root().to_path_buf());
    (dir, store)
}

/// The notes index over a second connection to the same database, which is how
/// the app reads it: the store owns the write path, the index owns the queries
/// (ADR-028 section 7).
fn index(dir: &TempDir) -> NotesIndexStore {
    NotesIndexStore::open(&dir.path().join("test.db")).expect("index db")
}

/// Note paths whose indexed text matches `raw`, through the same query policy
/// the sidebar search box uses.
fn search(dir: &TempDir, raw: &str) -> Vec<String> {
    let Some(query) = writ_core::search::to_prefix_match(raw) else {
        return Vec::new();
    };
    let terms = writ_core::search::search_terms(raw);
    index(dir)
        .search_hits(&query, &terms, 50)
        .expect("search")
        .into_iter()
        .filter_map(|hit| hit.path)
        .collect()
}

/// The key the index holds a note's file under.
/// Every note path the index holds a row for.
fn indexed_paths(dir: &TempDir) -> Vec<String> {
    index(dir)
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|(path, _, _)| path)
        .collect()
}

fn indexed_key(id: &str) -> String {
    writ_storage::notes_index::index_key(&note_path(id))
}

/// One notes folder for the whole test binary. Every note needs a file now
/// (ADR-028 §1), and ids are unique across these tests, so one folder keyed by
/// id costs less than threading a directory through every call site.
fn notes_root() -> &'static Path {
    static ROOT: OnceLock<TempDir> = OnceLock::new();
    ROOT.get_or_init(|| TempDir::new().expect("notes root"))
        .path()
}

fn note_path(id: &str) -> PathBuf {
    notes_root().join(format!("{id}.md"))
}

/// A note with a file behind it, created empty so it is openable.
fn make_doc(id: &str, title: &str) -> BufferDocument {
    let file = note_path(id);
    std::fs::write(&file, b"").expect("seed the note file");
    let mut doc = make_unwritten(id, title);
    doc.source_path = Some(file.to_string_lossy().into_owned());
    doc
}

/// A note that has not reached a file yet: what a new tab is until the first
/// keystroke.
fn make_unwritten(id: &str, title: &str) -> BufferDocument {
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
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    }
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
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
fn save_content_writes_the_file_and_no_copy() {
    let (dir, store) = setup();
    let doc = make_doc("buf-2", "Content Test");
    store.insert(&doc).expect("insert failed");
    store
        .save_content("buf-2", "Hello, file content!")
        .expect("save_content failed");

    assert_eq!(
        std::fs::read_to_string(note_path("buf-2")).unwrap(),
        "Hello, file content!"
    );
    assert!(
        is_empty_dir(&dir.path().join("buffers")),
        "a save copies the text nowhere"
    );
    assert_eq!(store.read_content("buf-2").unwrap(), "Hello, file content!");
}

#[test]
fn read_content_reads_the_file_and_not_a_copy() {
    // Structural: the store is built over a folder that does not exist, so
    // no copy of the text could be served even if one were wanted.
    let dir = TempDir::new().unwrap();
    let conn = open_database(&dir.path().join("test.db")).unwrap();
    run_migrations(&conn).unwrap();
    let store = BufferStore::new(conn, dir.path().join("absent"));

    let doc = make_doc("no-copies-1", "Only Copy");
    store.insert(&doc).unwrap();
    std::fs::write(note_path("no-copies-1"), "what the file says").unwrap();

    assert_eq!(
        store.read_content("no-copies-1").unwrap(),
        "what the file says"
    );
}

#[test]
fn read_content_of_a_note_with_no_file_is_empty() {
    let (_dir, store) = setup();
    store
        .insert(&make_unwritten("unwritten-1", "Untyped"))
        .unwrap();

    assert_eq!(store.read_content("unwritten-1").unwrap(), "");
}

#[test]
fn save_on_a_note_with_no_file_is_a_consistency_error() {
    let (_dir, store) = setup();
    store
        .insert(&make_unwritten("unwritten-2", "Untyped"))
        .unwrap();

    let err = store
        .save_content("unwritten-2", "typed")
        .expect_err("there is nowhere to write it")
        .to_string();
    assert!(err.contains("has no file"), "error: {err}");
}

#[test]
fn attach_source_path_makes_the_next_read_return_the_file() {
    let (_dir, store) = setup();
    store
        .insert(&make_unwritten("attach-1", "Untyped"))
        .unwrap();
    let file = note_path("attach-1");

    store
        .attach_source_path("attach-1", file.to_str().unwrap())
        .unwrap();
    store.save_content("attach-1", "first keystroke").unwrap();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "first keystroke");
    assert_eq!(store.read_content("attach-1").unwrap(), "first keystroke");
    assert!(
        store
            .attach_source_path("attach-1", "/elsewhere.md")
            .is_err(),
        "a note already has one file; repointing it is a move, not an attach"
    );
}

#[test]
fn a_binary_note_regenerates_its_hex_view_from_the_file() {
    let (_dir, store) = setup();
    let mut doc = make_doc("binary-1", "dump.bin");
    doc.read_only = true;
    doc.size_bytes = 4;
    store.insert(&doc).unwrap();
    // A NUL byte is what the binary sniff keys on; 0xdeadbeef alone would
    // read back as (lossy) text now that a read-only row is not assumed
    // binary just because it cannot be saved.
    std::fs::write(note_path("binary-1"), [0x00u8, 0xad, 0xbe, 0xef]).unwrap();

    let view = store.read_content("binary-1").unwrap();
    assert!(view.contains("00 ad be ef"), "hex view: {view}");
}

#[test]
fn a_read_only_text_note_reads_back_as_text_not_a_hex_view() {
    let (_dir, store) = setup();
    let mut doc = make_doc("readonly-text-1", "Third-party licences");
    doc.read_only = true;
    let content = "# Third-party notices\n\nMIT License\n";
    doc.size_bytes = content.len() as u64;
    store.insert(&doc).unwrap();
    std::fs::write(note_path("readonly-text-1"), content).unwrap();

    let view = store.read_content("readonly-text-1").unwrap();
    assert_eq!(
        view, content,
        "a read-only generated document is still text"
    );
}

#[test]
fn collect_buffer_contents_reads_the_files() {
    let (_dir, store) = setup();
    store.insert(&make_doc("collect-1", "One")).unwrap();
    store.insert(&make_unwritten("collect-2", "Two")).unwrap();
    std::fs::write(note_path("collect-1"), "on disk").unwrap();

    let collected = store.collect_buffer_contents().unwrap();

    assert_eq!(
        collected.get("collect-1").map(String::as_str),
        Some("on disk")
    );
    assert!(
        !collected.contains_key("collect-2"),
        "a note with no file has nothing to snapshot"
    );
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
fn delete_removes_the_row_but_never_the_file() {
    let (_dir, store) = setup();
    let doc = make_doc("buf-4", "Delete Me");
    store.insert(&doc).expect("insert failed");
    store
        .save_content("buf-4", "the note itself")
        .expect("save_content failed");

    store.delete("buf-4").expect("delete failed");

    assert!(store.get("buf-4").is_err(), "the row is gone");
    assert_eq!(
        std::fs::read_to_string(note_path("buf-4")).unwrap(),
        "the note itself",
        "closing a tab is not a request to delete the note"
    );
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
fn save_content_updates_the_notes_index() {
    let (dir, store) = setup();
    let doc = make_doc("fts1", "fts-test");
    store.insert(&doc).unwrap();
    store
        .save_content("fts1", "searchable content about rust programming")
        .unwrap();

    assert_eq!(search(&dir, "rust programming"), vec![indexed_key("fts1")]);
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
        search(&_dir, "uniqueterm").is_empty(),
        "deferred write must not be searchable before reindex",
    );
}

#[test]
fn reindex_buffer_makes_a_deferred_write_searchable() {
    let (dir, store) = setup();
    store.insert(&make_doc("deferred-2", "deferred")).unwrap();
    store
        .save_content_without_index("deferred-2", "alpha beta gamma")
        .unwrap();
    assert!(search(&dir, "beta").is_empty());

    store.reindex_buffer("deferred-2").expect("reindex failed");
    assert_eq!(search(&dir, "beta"), vec![indexed_key("deferred-2")]);
}

#[test]
fn reindex_buffer_reflects_latest_disk_content_after_coalesced_writes() {
    // Two deferred writes then a single reindex (the coalescing case): the
    // index must reflect only the latest bytes, never a stale intermediate.
    let (dir, store) = setup();
    store.insert(&make_doc("deferred-3", "deferred")).unwrap();
    store
        .save_content_without_index("deferred-3", "staleword first version")
        .unwrap();
    store
        .save_content_without_index("deferred-3", "freshword second version")
        .unwrap();

    store.reindex_buffer("deferred-3").expect("reindex failed");

    assert!(
        search(&dir, "staleword").is_empty(),
        "reindex must not surface a superseded intermediate",
    );
    assert_eq!(search(&dir, "freshword"), vec![indexed_key("deferred-3")]);
}

#[test]
fn delete_leaves_the_note_in_the_index_because_the_file_is_still_there() {
    // The index is keyed by file, not by row (ADR-028 section 7). Closing or
    // deleting a tab does not delete the note, so the note stays findable; it
    // leaves the index when its file leaves the folder, through the notes
    // watcher or the reconcile walk.
    let (dir, store) = setup();
    let doc = make_doc("orphan-1", "untitled");
    store.insert(&doc).unwrap();
    store.save_content("orphan-1", "foobar baseline").unwrap();
    assert_eq!(search(&dir, "foobar"), vec![indexed_key("orphan-1")]);

    store.delete("orphan-1").expect("delete failed");

    assert_eq!(
        search(&dir, "foobar"),
        vec![indexed_key("orphan-1")],
        "the note's file survives the row, and so does its index entry"
    );
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
fn rename_to_file_re_keys_the_note_in_the_index() {
    // A title rename that moves the file moves the index entry with it: the
    // old path goes and the new one comes in, in one call. Writ's own rename
    // is stamped into the watcher's ignore set, so the notes watcher will not
    // do this for it.
    let (dir, store) = setup();
    let doc = make_doc("ren-fts-a", "alpha");
    store.insert(&doc).unwrap();
    store.save_content("ren-fts-a", "body text").unwrap();
    assert_eq!(search(&dir, "body"), vec![indexed_key("ren-fts-a")]);

    let renamed = notes_root().join("beta.md");
    std::fs::rename(note_path("ren-fts-a"), &renamed).expect("rename the file");
    store
        .rename_to_file("ren-fts-a", renamed.to_str().unwrap(), "beta")
        .expect("rename_to_file");

    let key = writ_storage::notes_index::index_key(&renamed);
    assert_eq!(search(&dir, "body"), vec![key]);
    assert!(
        !search(&dir, "body").contains(&indexed_key("ren-fts-a")),
        "the note must not be in the index twice after a rename"
    );
}

#[test]
fn rename_to_file_moves_the_note_row_in_the_index() {
    // The search hit is derived from the note's row in `files`, and the row is
    // what a move has to carry: a row left under the old path is a note the
    // sidebar offers and nothing can open, and a second row under the new one
    // is the same note listed twice.
    let (dir, store) = setup();
    let doc = make_doc("ren-row-a", "alpha");
    store.insert(&doc).unwrap();
    store.save_content("ren-row-a", "body text").unwrap();
    let before = indexed_key("ren-row-a");
    assert!(indexed_paths(&dir).contains(&before));

    let renamed = notes_root().join("ren-row-b.md");
    std::fs::rename(note_path("ren-row-a"), &renamed).expect("rename the file");
    store
        .rename_to_file("ren-row-a", renamed.to_str().unwrap(), "ren-row-b")
        .expect("rename_to_file");

    let after = writ_storage::notes_index::index_key(&renamed);
    let paths = indexed_paths(&dir);
    assert!(
        paths.contains(&after),
        "the row did not follow the file: {paths:?}"
    );
    assert!(
        !paths.contains(&before),
        "the row under the path the file left is still there: {paths:?}"
    );
}

#[test]
fn rename_alone_leaves_the_index_untouched() {
    // The index is labelled by file name. A title that changes without the
    // file moving changes nothing it holds.
    let (dir, store) = setup();
    let doc = make_doc("ren-fts-b", "alpha");
    store.insert(&doc).unwrap();
    store.save_content("ren-fts-b", "body text").unwrap();

    store.rename("ren-fts-b", "beta").unwrap();

    assert_eq!(store.get("ren-fts-b").unwrap().title, "beta");
    assert_eq!(search(&dir, "body"), vec![indexed_key("ren-fts-b")]);
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
fn delete_many_removes_rows_and_index_entries_but_no_files() {
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
    for id in ["dm-a", "dm-b", "dm-c"] {
        assert!(
            note_path(id).exists(),
            "clearing rows never deletes the notes themselves"
        );
    }
    assert!(is_empty_dir(&dir.path().join("buffers")));
    assert_eq!(
        search(&dir, "needle").len(),
        3,
        "the index follows the files, and clearing rows deletes no file"
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

    // Refuse the second row's delete from inside the database, so the batch
    // fails after the first delete has already run. The failure must roll the
    // whole transaction back, leaving both rows intact rather than deleting
    // the first before the second errors.
    second_conn(&dir)
        .execute_batch(
            "CREATE TRIGGER refuse_dm_tx_2 BEFORE DELETE ON buffers
             WHEN OLD.id = 'dm-tx-2'
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
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
    let (dir, store) = setup();
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
        search(&dir, "persist").len(),
        2,
        "the index must be untouched when the batch aborts"
    );
}

#[test]
fn delete_many_is_noop_on_empty_input() {
    let (_dir, store) = setup();
    let doc = make_doc("dm-solo", "solo");
    store.insert(&doc).unwrap();

    store
        .delete_many(&[])
        .expect("empty delete_many is a no-op");

    assert!(store.get("dm-solo").is_ok());
}

#[test]
fn a_generated_document_never_enters_the_index() {
    // ADR-028 section 1: a document Writ wrote rather than the user is not
    // something search may return, no matter which path reaches it.
    let (dir, store) = setup();
    let mut doc = make_doc("rf-notice", "Third-party licences");
    doc.read_only = true;
    std::fs::write(
        doc.source_path.as_ref().unwrap(),
        "permission is hereby granted to any dweomerword holder",
    )
    .unwrap();
    store.open_from_path_unindexed(&doc).unwrap();

    assert!(
        search(&dir, "dweomerword").is_empty(),
        "opening a generated document must not index it"
    );

    store.reindex_buffer("rf-notice").expect("reindex");
    assert!(
        search(&dir, "dweomerword").is_empty(),
        "neither may a reindex"
    );
}

#[test]
fn rename_preserves_content_searchability() {
    let (dir, store) = setup();
    let doc = make_doc("ren-fts-c", "alpha");
    store.insert(&doc).unwrap();
    store
        .save_content("ren-fts-c", "lorem ipsum dolor sit amet")
        .unwrap();

    store.rename("ren-fts-c", "beta").unwrap();

    assert_eq!(search(&dir, "ipsum"), vec![indexed_key("ren-fts-c")]);
}

#[test]
fn find_empty_scratch_active_returns_none_on_empty_store() {
    let (_dir, store) = setup();
    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn find_empty_scratch_active_returns_an_active_note_with_no_file() {
    let (_dir, store) = setup();
    store
        .insert(&make_unwritten("scratch-1", "2026-08-28"))
        .unwrap();

    let found = store.find_empty_scratch_active().unwrap();
    assert_eq!(found.map(|d| d.id), Some("scratch-1".to_string()));
}

#[test]
fn find_empty_scratch_active_skips_a_note_that_has_a_file() {
    let (_dir, store) = setup();
    store.insert(&make_doc("scratch-2", "2026-08-28")).unwrap();

    assert!(
        store.find_empty_scratch_active().unwrap().is_none(),
        "a note reaches a file on the first keystroke, so having one means it holds text"
    );
}

#[test]
fn find_empty_scratch_active_skips_history_notes() {
    let (_dir, store) = setup();
    store
        .insert(&make_unwritten("scratch-3", "2026-08-28"))
        .unwrap();
    store.close("scratch-3").unwrap();

    assert!(store.find_empty_scratch_active().unwrap().is_none());
}

#[test]
fn reclaim_empty_scratch_deletes_notes_with_no_file_in_any_status() {
    let (_dir, store) = setup();

    store
        .insert(&make_unwritten("re-active-empty", "2026-08-28"))
        .unwrap();
    store
        .insert(&make_unwritten("re-history-empty", "2026-08-28"))
        .unwrap();
    store.close("re-history-empty").unwrap();
    store.insert(&make_doc("re-content", "Kept")).unwrap();
    store.save_content("re-content", "keep me").unwrap();

    let count = store.reclaim_empty_scratch().unwrap();

    assert_eq!(count, 2);
    assert!(store.get("re-active-empty").is_err());
    assert!(store.get("re-history-empty").is_err());
    assert!(store.get("re-content").is_ok());
}

#[test]
fn reclaim_empty_scratch_keeps_every_note_that_has_a_file() {
    let (_dir, store) = setup();
    store.insert(&make_doc("kept-named", "Important")).unwrap();
    store.insert(&make_doc("kept-sourced", "real.txt")).unwrap();

    assert_eq!(store.reclaim_empty_scratch().unwrap(), 0);
    assert!(store.get("kept-named").is_ok());
    assert!(store.get("kept-sourced").is_ok());
}

#[test]
fn reclaim_empty_scratch_never_deletes_a_file() {
    let (_dir, store) = setup();
    store.insert(&make_doc("re-file", "Kept")).unwrap();
    store.save_content("re-file", "").unwrap();

    store.reclaim_empty_scratch().unwrap();

    assert!(store.get("re-file").is_ok(), "the note has a file");
    assert!(note_path("re-file").exists());
}

// Custom doc with a caller-chosen filename, to exercise legacy rows whose
// mirror filename predates the UUID-derived naming (audit blocker #53.7).
fn make_doc_with_filename(id: &str, title: &str, filename: &str) -> BufferDocument {
    let mut doc = make_unwritten(id, title);
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
    assert_eq!(
        std::fs::read_to_string(buffers.join("legacy-1.txt")).unwrap(),
        "legacy content",
        "the copy is only moved here; the notes migration is what places it"
    );
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
