use chrono::{Duration, Utc};
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::buffer::manager::BufferManager;
use writ_core::notes::line_ending::LineEnding;
use writ_core::recovery::{
    journal_step, resolve_recovery, JournalStep, RecoveryResolution, SaveLanding,
};
use writ_storage::buffer_store::{BufferStore, RecoveredText};
use writ_storage::consistency::ConsistencyChecker;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;
use writ_storage::recovery::dirty_shutdown::check_dirty_shutdown;
use writ_storage::recovery::journal::UnsavedJournal;
use writ_storage::recovery::snapshot::SnapshotManager;

/// Attaches a file inside `dir` to a note, the way the first keystroke does.
fn attach_note(store: &BufferStore, dir: &TempDir, id: &str) -> std::path::PathBuf {
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    let file = notes.join(format!("{id}.md"));
    store
        .attach_source_path(id, file.to_str().expect("path"))
        .expect("attach");
    file
}

fn setup_store() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let store = open_store(&dir);
    (dir, store)
}

/// A store over `dir`'s database, as a launch opens it.
fn open_store(dir: &TempDir) -> BufferStore {
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    BufferStore::new(conn, buffers_dir)
}

fn setup_conn() -> (TempDir, rusqlite::Connection) {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    (dir, conn)
}

fn second_connection(dir: &TempDir) -> rusqlite::Connection {
    open_database(&dir.path().join("writ.db")).expect("open db")
}

/// A note the store holds, with a file in `dir` when `on_disk` is given.
fn note(store: &BufferStore, dir: &TempDir, id: &str, on_disk: Option<&str>) -> BufferDocument {
    let mut mgr = BufferManager::new();
    let mut doc = mgr.create_buffer(Some(id.to_string())).expect("create");
    doc.id = id.to_string();
    store.insert(&doc).expect("insert");
    if let Some(text) = on_disk {
        let file = attach_note(store, dir, id);
        std::fs::write(&file, text).expect("write the note");
    }
    store.get(id).expect("the row")
}

fn journaled(store: &BufferStore) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = store
        .unsaved_text()
        .expect("read the journal")
        .entries
        .into_iter()
        .map(|entry| (entry.buffer_id, entry.text))
        .collect();
    entries.sort();
    entries
}

fn snapshot_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM session_snapshots", [], |row| {
        row.get(0)
    })
    .expect("count")
}

// --- session markers ---

#[test]
fn a_marker_holds_no_text() {
    let (dir, mut store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    assert!(store.mark_session_running().expect("mark"));
    let conn = second_connection(&dir);
    let snap = SnapshotManager::new(&conn)
        .latest_snapshot()
        .expect("latest")
        .expect("some");
    assert!(!snap.is_clean);
    assert_eq!(snap.state_json, serde_json::json!({}));
}

#[test]
fn snapshot_clean_flag_roundtrip() {
    let (_dir, conn) = setup_conn();
    let mgr = SnapshotManager::new(&conn);
    mgr.write_snapshot(&serde_json::json!({}), true)
        .expect("write");
    let snap = mgr.latest_snapshot().expect("latest").expect("some");
    assert!(snap.is_clean);
}

#[test]
fn snapshot_prune_keeps_bounded_count() {
    let (_dir, conn) = setup_conn();
    let mgr = SnapshotManager::new(&conn);

    // Write more than MAX_SNAPSHOTS (5)
    for _ in 0..8 {
        mgr.write_snapshot(&serde_json::json!({}), false)
            .expect("write");
    }

    assert_eq!(
        snapshot_count(&conn),
        5,
        "should have pruned to MAX_SNAPSHOTS"
    );
}

#[test]
fn no_snapshot_means_clean() {
    let (_dir, conn) = setup_conn();
    let is_dirty = check_dirty_shutdown(&conn).expect("check");
    assert!(!is_dirty, "no snapshot should not report dirty");
}

#[test]
fn a_running_marker_reads_as_a_dirty_shutdown() {
    let (dir, mut store) = setup_store();
    store.mark_session_running().expect("mark");
    assert!(
        check_dirty_shutdown(&second_connection(&dir)).expect("check"),
        "a session that never wrote its clean marker should read as dirty"
    );
}

#[test]
fn a_clean_marker_clears_the_dirty_flag() {
    let (dir, mut store) = setup_store();
    store.mark_session_running().expect("running");
    store.mark_session_clean().expect("clean");
    assert_eq!(snapshot_count(&second_connection(&dir)), 2);
    assert!(
        !store.is_dirty_shutdown().expect("dirty check"),
        "the clean marker is the last thing a quit writes, whatever came before it"
    );
}

#[test]
fn the_running_marker_is_written_once_per_store() {
    let (dir, mut store) = setup_store();
    assert!(
        store.mark_session_running().expect("first beat"),
        "the first beat has written nothing yet and must write"
    );
    assert!(
        !store.mark_session_running().expect("second beat"),
        "a later beat has nothing new to say"
    );
    assert_eq!(snapshot_count(&second_connection(&dir)), 1);
}

// --- the journal ---

#[test]
fn text_that_never_reached_a_file_survives_an_exit_nothing_cleaned_up_after() {
    let dir = TempDir::new().expect("temp dir");
    {
        let mut store = open_store(&dir);
        note(&store, &dir, "locked", Some("what the file held\n"));
        store
            .mark_session_running()
            .expect("the session is running");
        assert!(store
            .record_unsaved_text("locked", "typed after the file was locked\n", Utc::now())
            .expect("journal"));
        // Dropped with no clean marker and nothing written on the way out,
        // which is all a killed process or a power cut leaves behind.
    }

    let relaunched = open_store(&dir);
    assert!(relaunched.is_dirty_shutdown().expect("dirty check"));
    assert_eq!(
        journaled(&relaunched),
        vec![(
            "locked".to_string(),
            "typed after the file was locked\n".to_string()
        )]
    );
}

#[test]
fn the_journal_holds_what_the_editor_had_not_what_the_file_holds() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    store
        .record_unsaved_text("note-1", "typed", Utc::now())
        .expect("journal");

    // Written by something other than Writ: the journal reads no file, so
    // what it keeps is the text nothing else holds.
    std::fs::write(
        dir.path().join("notes").join("note-1.md"),
        "text somebody else wrote",
    )
    .expect("external write");

    assert_eq!(
        journaled(&store),
        vec![("note-1".to_string(), "typed".to_string())]
    );
}

#[test]
fn a_later_save_that_fails_replaces_the_text() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    store
        .record_unsaved_text("note-1", "first", Utc::now())
        .expect("first");
    store
        .record_unsaved_text("note-1", "second", Utc::now())
        .expect("second");
    assert_eq!(
        journaled(&store),
        vec![("note-1".to_string(), "second".to_string())]
    );
}

#[test]
fn recording_the_same_text_again_rewrites_nothing() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    let first = Utc::now() - Duration::seconds(60);
    store
        .record_unsaved_text("note-1", "same", first)
        .expect("first");
    store
        .record_unsaved_text("note-1", "same", Utc::now())
        .expect("again");

    let entries = store.unsaved_text().expect("read").entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].recorded_at.timestamp(),
        first.timestamp(),
        "a retry of the same text must not rewrite the entry"
    );
}

#[test]
fn clearing_drops_that_note_alone() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("one"));
    note(&store, &dir, "note-2", Some("two"));
    store
        .record_unsaved_text("note-1", "typed one", Utc::now())
        .expect("one");
    store
        .record_unsaved_text("note-2", "typed two", Utc::now())
        .expect("two");

    assert!(store.clear_unsaved_text("note-1").expect("clear"));
    assert!(
        !store.clear_unsaved_text("note-1").expect("clear again"),
        "there was nothing left to clear"
    );
    assert_eq!(
        journaled(&store),
        vec![("note-2".to_string(), "typed two".to_string())]
    );
}

/// What the journal holds for `id`, oldest first, with whether a tab holds it.
fn entries_of(store: &BufferStore, id: &str) -> Vec<(String, bool)> {
    store
        .unsaved_text()
        .expect("read the journal")
        .entries
        .into_iter()
        .filter(|entry| entry.buffer_id == id)
        .map(|entry| (entry.text, entry.in_tab))
        .collect()
}

#[test]
fn reopening_a_closed_note_releases_its_tabs_entry() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    store
        .record_unsaved_text("note-1", "typed before the close", Utc::now())
        .expect("journal");
    store.close("note-1").expect("close");
    store.restore("note-1").expect("reopen");

    assert_eq!(
        entries_of(&store, "note-1"),
        vec![("typed before the close".to_string(), false)]
    );
    assert!(
        !store.clear_unsaved_text("note-1").expect("clear"),
        "a save of the reopened tab has no entry of its own to clear"
    );

    store
        .record_unsaved_text("note-1", "typed after the reopen", Utc::now())
        .expect("journal");
    store
        .record_unsaved_text("note-1", "typed after the reopen, again", Utc::now())
        .expect("journal");
    assert_eq!(
        entries_of(&store, "note-1"),
        vec![
            ("typed before the close".to_string(), false),
            ("typed after the reopen, again".to_string(), true),
        ],
        "the new tab's text replaces its own entry and leaves the released one"
    );

    assert!(store.clear_unsaved_text("note-1").expect("clear"));
    assert_eq!(
        entries_of(&store, "note-1"),
        vec![("typed before the close".to_string(), false)]
    );
}

#[test]
fn reopening_a_note_that_is_open_keeps_its_tabs_entry() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    store
        .record_unsaved_text("note-1", "typed", Utc::now())
        .expect("journal");
    store.restore("note-1").expect("already open");
    assert_eq!(
        entries_of(&store, "note-1"),
        vec![("typed".to_string(), true)]
    );
}

#[test]
fn reopening_a_note_with_no_row_fails() {
    let (_dir, store) = setup_store();
    assert!(store.restore("no-such-note").is_err());
}

#[test]
fn a_launch_releases_every_tabs_entry() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("one"));
    note(&store, &dir, "note-2", Some("two"));
    store
        .record_unsaved_text("note-1", "typed one", Utc::now())
        .expect("one");
    store
        .record_unsaved_text("note-2", "typed two", Utc::now())
        .expect("two");

    assert_eq!(store.release_unsaved_text().expect("release"), 2);
    assert_eq!(
        entries_of(&store, "note-1"),
        vec![("typed one".to_string(), false)]
    );
    assert_eq!(
        entries_of(&store, "note-2"),
        vec![("typed two".to_string(), false)]
    );
    assert!(!store.clear_unsaved_text("note-1").expect("clear"));
    assert_eq!(store.unsaved_text_count().expect("count"), 2);
    assert_eq!(store.release_unsaved_text().expect("again"), 0);
}

#[test]
fn an_entry_given_back_to_its_tab_is_the_one_a_save_settles() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    store
        .record_unsaved_text("note-1", "older", Utc::now())
        .expect("older");
    store.close("note-1").expect("close");
    store.restore("note-1").expect("reopen");
    store
        .record_unsaved_text("note-1", "newer", Utc::now())
        .expect("newer");
    store.release_unsaved_text().expect("a launch");

    let entries = store.unsaved_text().expect("read").entries;
    let (older, newer) = (entries[0].entry_id, entries[1].entry_id);
    assert!(store.give_unsaved_text_to_tab(newer).expect("give"));
    assert!(
        store.give_unsaved_text_to_tab(older).is_err(),
        "a note's tab has one entry at most"
    );
    assert!(!store
        .give_unsaved_text_to_tab(i64::MAX)
        .expect("no such entry"));

    assert!(store.clear_unsaved_text("note-1").expect("clear"));
    assert_eq!(
        entries_of(&store, "note-1"),
        vec![("older".to_string(), false)]
    );
}

#[test]
fn a_launch_removes_the_entry_it_put_on_disk_alone() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    store
        .record_unsaved_text("note-1", "older", Utc::now())
        .expect("older");
    store.close("note-1").expect("close");
    store.restore("note-1").expect("reopen");
    store
        .record_unsaved_text("note-1", "newer", Utc::now())
        .expect("newer");

    let older = store.unsaved_text().expect("read").entries[0].entry_id;
    assert!(store.remove_unsaved_text_entry(older).expect("remove"));
    assert!(!store
        .remove_unsaved_text_entry(older)
        .expect("remove again"));
    assert_eq!(
        entries_of(&store, "note-1"),
        vec![("newer".to_string(), true)]
    );
}

#[test]
fn the_journal_releases_entries_gives_one_back_and_removes_one() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("one"));
    note(&store, &dir, "note-2", Some("two"));
    let conn = second_connection(&dir);
    let journal = UnsavedJournal::new(&conn);
    journal
        .record("note-1", None, "typed one", Utc::now())
        .expect("one");
    journal
        .record("note-2", None, "typed two", Utc::now())
        .expect("two");

    assert!(journal.release("note-1").expect("release"));
    assert!(!journal.release("note-1").expect("nothing left in its tab"));
    assert!(
        !journal.clear("note-1").expect("clear"),
        "a released entry is no save's to clear"
    );
    assert_eq!(journal.release_all().expect("release the rest"), 1);

    let entries = journal.read().expect("read").entries;
    assert!(entries.iter().all(|entry| !entry.in_tab));
    let entry_of = |id: &str| {
        entries
            .iter()
            .find(|entry| entry.buffer_id == id)
            .expect("the entry")
            .entry_id
    };
    assert!(journal.give_to_tab(entry_of("note-1")).expect("give"));
    assert!(journal.clear("note-1").expect("clear the tab's entry"));
    assert!(journal.remove(entry_of("note-2")).expect("remove"));
    assert_eq!(journal.count().expect("count"), 0);
}

#[test]
fn every_entry_goes_with_its_note() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("one"));
    store
        .record_unsaved_text("note-1", "typed", Utc::now())
        .expect("journal");
    store.close("note-1").expect("close");
    store.restore("note-1").expect("reopen");
    store
        .record_unsaved_text("note-1", "typed after the reopen", Utc::now())
        .expect("journal");
    assert_eq!(store.unsaved_text_count().expect("count"), 2);
    store.delete("note-1").expect("delete the note");
    assert_eq!(store.unsaved_text_count().expect("count"), 0);
}

#[test]
fn an_entry_needs_a_note() {
    let (_dir, store) = setup_store();
    assert!(store
        .record_unsaved_text("no-such-note", "typed", Utc::now())
        .is_err());
    assert_eq!(store.unsaved_text_count().expect("count"), 0);
}

#[test]
fn a_read_only_note_is_not_journaled() {
    let (dir, store) = setup_store();
    let mut mgr = BufferManager::new();
    let mut doc = mgr.create_buffer(Some("generated".into())).expect("create");
    doc.read_only = true;
    let file = dir.path().join("generated.md");
    std::fs::write(&file, "not anybody's typing").expect("write");
    doc.source_path = Some(file.to_string_lossy().into_owned());
    store.insert(&doc).expect("insert");

    assert!(!store
        .record_unsaved_text(&doc.id, "a hex view", Utc::now())
        .expect("asked"));
    assert_eq!(store.unsaved_text_count().expect("count"), 0);
}

fn synchronous(conn: &rusqlite::Connection) -> i64 {
    conn.pragma_query_value(None, "synchronous", |row| row.get(0))
        .expect("level")
}

#[test]
fn a_journal_write_puts_its_connection_back_at_the_level_it_had() {
    let (dir, store) = setup_store();
    note(&store, &dir, "note-1", Some("one"));
    let conn = second_connection(&dir);
    // OFF is 0, NORMAL 1, FULL 2. Starting from OFF tells a level put back
    // apart from one reset to the NORMAL every connection opens at.
    conn.pragma_update(None, "synchronous", "off")
        .expect("set the level");
    let journal = UnsavedJournal::new(&conn);

    journal
        .record("note-1", None, "typed", Utc::now())
        .expect("journal");
    assert_eq!(synchronous(&conn), 0, "after a write that landed");

    assert!(
        journal
            .record("no-such-note", None, "typed", Utc::now())
            .is_err(),
        "an entry needs a note"
    );
    assert_eq!(synchronous(&conn), 0, "after a write that failed");
}

#[test]
fn holds_answers_for_an_entry_whether_or_not_it_decodes() {
    let (dir, store) = setup_store();
    note(&store, &dir, "typed-into", Some("one"));
    note(&store, &dir, "untouched", Some("two"));
    note(&store, &dir, "unreadable", Some("three"));
    let conn = second_connection(&dir);
    let journal = UnsavedJournal::new(&conn);
    assert!(!journal.holds("typed-into").expect("before the record"));

    store
        .record_unsaved_text("typed-into", "typed", Utc::now())
        .expect("journal");
    conn.execute(
        "INSERT INTO unsaved_text (buffer_id, path, text, recorded_at)
         VALUES ('unreadable', NULL, CAST(X'FFFE' AS TEXT), '2026-10-13T00:00:00+00:00')",
        [],
    )
    .expect("insert text that is not UTF-8");

    assert!(journal.holds("typed-into").expect("recorded"));
    assert!(!journal.holds("untouched").expect("never recorded"));
    assert!(
        journal.holds("unreadable").expect("undecodable"),
        "an entry that does not decode still holds the note's text"
    );

    store.clear_unsaved_text("typed-into").expect("clear");
    assert!(!journal.holds("typed-into").expect("cleared"));
}

#[test]
fn an_undecodable_row_is_reported_and_kept() {
    let (dir, store) = setup_store();
    note(&store, &dir, "good", Some("one"));
    note(&store, &dir, "bad-text", Some("two"));
    note(&store, &dir, "bad-time", Some("three"));
    store
        .record_unsaved_text("good", "typed", Utc::now())
        .expect("journal");
    let conn = second_connection(&dir);
    conn.execute(
        "INSERT INTO unsaved_text (buffer_id, path, text, recorded_at)
         VALUES ('bad-text', NULL, CAST(X'FFFE' AS TEXT), '2026-10-13T00:00:00+00:00')",
        [],
    )
    .expect("insert text that is not UTF-8");
    conn.execute(
        "INSERT INTO unsaved_text (buffer_id, path, text, recorded_at)
         VALUES ('bad-time', NULL, 'typed', 'not a time')",
        [],
    )
    .expect("insert a time that does not parse");

    let read = store.unsaved_text().expect("the rest is still read");
    assert_eq!(read.entries.len(), 1);
    assert_eq!(read.entries[0].buffer_id, "good");
    assert_eq!(read.undecodable.len(), 2, "{:?}", read.undecodable);
    assert!(
        read.undecodable[0].reason.starts_with("text: ")
            || read.undecodable[1].reason.starts_with("text: ")
    );
    assert!(
        read.undecodable[0].reason.starts_with("recorded_at: ")
            || read.undecodable[1].reason.starts_with("recorded_at: ")
    );

    // Restored and cleared, the readable entry goes. The two that could not
    // be read are still there.
    store.clear_unsaved_text("good").expect("clear");
    assert_eq!(store.unsaved_text_count().expect("count"), 2);
    assert_eq!(store.unsaved_text().expect("read").undecodable.len(), 2);
}

#[test]
fn a_note_with_no_file_and_journaled_text_is_neither_reused_nor_reclaimed() {
    let (dir, store) = setup_store();
    // Typed into, and its first file could not be made.
    note(&store, &dir, "typed-into", None);
    store
        .record_unsaved_text("typed-into", "the only copy", Utc::now())
        .expect("journal");
    // Never typed into.
    note(&store, &dir, "empty", None);

    let reusable = store
        .find_empty_scratch_active()
        .expect("find")
        .expect("the empty one");
    assert_eq!(reusable.id, "empty");

    assert_eq!(store.reclaim_empty_scratch().expect("reclaim"), 1);
    assert!(store.get("typed-into").is_ok(), "the row was reclaimed");
    assert!(store.get("empty").is_err());
    assert_eq!(
        journaled(&store),
        vec![("typed-into".to_string(), "the only copy".to_string())]
    );
}

#[test]
fn a_closed_note_whose_save_was_refused_comes_back_on_the_next_launch() {
    // The whole path, in order: a note's save is refused, the tab is closed
    // (the row moves to history), and the next launch has to find it.
    let dir = TempDir::new().expect("temp dir");
    {
        let store = open_store(&dir);
        note(&store, &dir, "refused", Some("what the file still holds"));
        store
            .record_unsaved_text("refused", "the text the file refused", Utc::now())
            .expect("journal");
        store.close("refused").expect("close the tab");
        assert!(store
            .list_by_status(BufferStatus::Active)
            .expect("list")
            .is_empty());
    }

    let relaunched = open_store(&dir);
    assert_eq!(
        journaled(&relaunched),
        vec![(
            "refused".to_string(),
            "the text the file refused".to_string()
        )]
    );
}

#[test]
fn a_quit_that_left_unsaved_text_offers_it_after_a_clean_shutdown_too() {
    let (dir, mut store) = setup_store();
    note(&store, &dir, "note-1", Some("on disk"));
    store.mark_session_running().expect("running");
    store
        .record_unsaved_text("note-1", "typed", Utc::now())
        .expect("journal");
    store.mark_session_clean().expect("the quit");

    assert!(!store.is_dirty_shutdown().expect("dirty check"));
    assert_eq!(
        journaled(&store),
        vec![("note-1".to_string(), "typed".to_string())],
        "the journal is read on every launch, not only after a crash"
    );
}

/// Boot-path contract (#71): `AppState::initialize` runs dirty-shutdown
/// detection, the journal read, and the consistency check against the SAME
/// store instance, in that order. This is only possible because
/// `ConsistencyChecker` borrows the store rather than consuming it; a
/// regression to owning would break this sequence at compile time.
#[test]
fn boot_sequence_runs_recovery_then_consistency_on_one_store() {
    let (dir, mut store) = setup_store();
    note(&store, &dir, "buf-crash", Some("on disk"));
    store
        .record_unsaved_text("buf-crash", "unsaved work", Utc::now())
        .expect("journal");
    store.mark_session_running().expect("running");

    // 1. detection
    assert!(store.is_dirty_shutdown().expect("dirty check"));
    // 2. the journal (same store)
    assert_eq!(store.unsaved_text().expect("read").entries.len(), 1);
    // 3. consistency check (same store, borrowed)
    let report = ConsistencyChecker::new(&store)
        .check()
        .expect("consistency check");
    assert!(
        report.missing_files.is_empty(),
        "freshly written buffers must not report as missing: {:?}",
        report.missing_files
    );
}

#[test]
fn a_recovered_note_with_no_file_gets_one_before_its_text_is_written() {
    // What startup does with a note that crashed before its first keystroke
    // reached a file: there is nowhere to write the recovered text until one
    // is attached, and the attach is what makes the text durable.
    let (dir, store) = setup_store();
    let mut mgr = BufferManager::new();
    let doc = mgr.create_buffer(None).expect("create");
    store.insert(&doc).expect("insert");

    assert!(
        store.save_content(&doc.id, "recovered text").is_err(),
        "a note with no file has nowhere to put the text"
    );

    let file = attach_note(&store, &dir, &doc.id);
    store
        .save_content(&doc.id, "recovered text")
        .expect("save after the file is attached");

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "recovered text");
    assert_eq!(store.read_content(&doc.id).unwrap(), "recovered text");
}

// --- which failed saves are journaled ---

#[test]
fn every_refusal_that_left_the_text_with_the_editor_is_recorded() {
    use std::io::{Error, ErrorKind};

    let left_with_the_editor = [
        (
            "not downloaded",
            StorageError::SourceNotDownloaded {
                path: "/notes/evicted.md".into(),
            },
        ),
        (
            "timed out",
            StorageError::Io(Error::new(
                ErrorKind::TimedOut,
                "the share stopped answering",
            )),
        ),
        (
            "unmounted",
            StorageError::Io(Error::new(ErrorKind::NotFound, "the volume is gone")),
        ),
        (
            "read-only file",
            StorageError::DestinationReadOnly {
                path: "/notes/locked.md".into(),
            },
        ),
        (
            "folder not writable",
            StorageError::DestinationFolderNotWritable {
                path: "/notes/locked/a.md".into(),
            },
        ),
        (
            "hard-linked",
            StorageError::HardLinkedDestination {
                path: "/notes/linked.md".into(),
                links: 2,
            },
        ),
        (
            "permission denied",
            StorageError::Io(Error::new(ErrorKind::PermissionDenied, "denied")),
        ),
        (
            "changed on disk, copy failed",
            StorageError::SourceChangedOnDisk {
                path: "/notes/a.md".into(),
                disk_hash: "abc".into(),
                conflict_copy: None,
            },
        ),
    ];
    for (case, error) in left_with_the_editor {
        assert_eq!(error.save_landing(), SaveLanding::NotLanded, "{case}");
        assert_eq!(
            journal_step(error.save_landing()),
            JournalStep::Record,
            "{case}"
        );
    }

    let beside = StorageError::SourceChangedOnDisk {
        path: "/notes/a.md".into(),
        disk_hash: "abc".into(),
        conflict_copy: Some("/notes/a (conflict 2026-10-13 10.00.00).md".into()),
    };
    assert_eq!(beside.save_landing(), SaveLanding::WrittenBeside);
    assert_eq!(
        journal_step(beside.save_landing()),
        JournalStep::Clear,
        "a copy beside the note is a file holding the text"
    );
}

// --- restoring journaled text ---

#[test]
fn journaled_text_goes_back_with_the_notes_own_line_ending() {
    let (dir, store) = setup_store();
    let doc = note(&store, &dir, "crlf", Some("one\r\ntwo\r\n"));
    store
        .set_line_ending(&doc.id, LineEnding::CrLf)
        .expect("the note is CRLF");

    // The editor's text is LF whatever the file uses.
    let outcome = store
        .restore_recovered_content(&doc.id, "one\ntwo\nthree\n", None, None)
        .expect("restore");
    let RecoveredText::SetAside { copy, .. } = outcome else {
        panic!("the file holds other text, so the journaled text goes beside it");
    };
    assert_eq!(
        std::fs::read_to_string(&copy).expect("read the copy"),
        "one\r\ntwo\r\nthree\r\n"
    );
}

#[test]
fn a_file_holding_the_journaled_text_in_its_own_endings_is_left_alone() {
    let (dir, store) = setup_store();
    let doc = note(&store, &dir, "crlf", Some("one\r\ntwo\r\n"));
    store
        .set_line_ending(&doc.id, LineEnding::CrLf)
        .expect("the note is CRLF");

    let outcome = store
        .restore_recovered_content(&doc.id, "one\ntwo\n", None, None)
        .expect("restore");
    assert!(matches!(outcome, RecoveredText::Restored(_)), "{outcome:?}");
    let copies = std::fs::read_dir(dir.path().join("notes"))
        .expect("read_dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("(recovered "))
        .count();
    assert_eq!(copies, 0, "the file already holds the text");
}

// --- the 0.5 snapshot, at the first launch after the upgrade ---

/// A snapshot row as 0.5 wrote it, `secs` seconds from now.
fn legacy_snapshot(conn: &rusqlite::Connection, id: &str, state: &str, secs: i64, clean: bool) {
    let at = (Utc::now() + Duration::seconds(secs))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    conn.execute(
        "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
         VALUES (?1, 1, ?2, ?3, ?4)",
        rusqlite::params![id, state, at, i64::from(clean)],
    )
    .expect("insert the snapshot");
}

fn snapshot_state(conn: &rusqlite::Connection, id: &str) -> String {
    conn.query_row(
        "SELECT state_json FROM session_snapshots WHERE id = ?1",
        rusqlite::params![id],
        |row| row.get(0),
    )
    .expect("the snapshot is there")
}

/// A note whose row was last stamped `secs` seconds from now.
fn stamped_note(store: &BufferStore, dir: &TempDir, id: &str, secs: i64) {
    stamped_row(store, dir, id, secs, false);
}

/// A read-only row last stamped `secs` seconds from now: a generated document
/// such as the third-party licences, which 0.5 opened in a tab.
fn stamped_generated_document(store: &BufferStore, dir: &TempDir, id: &str, secs: i64) {
    stamped_row(store, dir, id, secs, true);
}

fn stamped_row(store: &BufferStore, dir: &TempDir, id: &str, secs: i64, read_only: bool) {
    let at = Utc::now() + Duration::seconds(secs);
    let mut mgr = BufferManager::new();
    let mut doc = mgr.create_buffer(Some(id.to_string())).expect("create");
    doc.id = id.to_string();
    doc.created_at = at;
    doc.updated_at = at;
    doc.read_only = read_only;
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes dir");
    let file = notes.join(format!("{id}.md"));
    std::fs::write(&file, "on disk").expect("write");
    doc.source_path = Some(file.to_string_lossy().into_owned());
    store.insert(&doc).expect("insert");
}

#[test]
fn the_latest_unclean_snapshot_of_0_5_moves_into_the_journal() {
    let (dir, store) = setup_store();
    stamped_note(&store, &dir, "kept", -30);
    let conn = second_connection(&dir);
    legacy_snapshot(
        &conn,
        "shutdown",
        r#"{"buffers":{"kept":"the text the quit was handed","gone":"a note with no row"}}"#,
        0,
        false,
    );

    let carried = store.carry_over_legacy_snapshots().expect("carry over");
    assert_eq!(carried.journaled, 1, "{carried:?}");
    assert_eq!(carried.emptied, 1);

    let entries = store.unsaved_text().expect("read").entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].buffer_id, "kept");
    assert_eq!(entries[0].text, "the text the quit was handed");
    assert_eq!(
        entries[0].path.as_deref(),
        Some(
            dir.path()
                .join("notes")
                .join("kept.md")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        snapshot_state(&conn, "shutdown"),
        "{}",
        "writ.db keeps no second copy of the text"
    );
}

#[test]
fn a_read_only_note_in_the_last_0_5_snapshot_is_not_journaled() {
    let (dir, store) = setup_store();
    stamped_note(&store, &dir, "typed-into", -30);
    stamped_generated_document(&store, &dir, "licences", -30);
    let conn = second_connection(&dir);
    legacy_snapshot(
        &conn,
        "heartbeat",
        r#"{"buffers":{"typed-into":"typed","licences":"the generated text"}}"#,
        0,
        false,
    );

    let carried = store.carry_over_legacy_snapshots().expect("carry over");
    assert_eq!(carried.journaled, 1, "{carried:?}");
    assert_eq!(carried.emptied, 1);
    // The launch refuses to write into a read-only row, so an entry for one
    // would never clear.
    assert_eq!(
        journaled(&store),
        vec![("typed-into".to_string(), "typed".to_string())]
    );
    assert_eq!(snapshot_state(&conn, "heartbeat"), "{}");
}

#[test]
fn an_entry_older_than_its_note_stays_behind_as_0_5_left_it() {
    let (dir, store) = setup_store();
    // Writ saved the note after the snapshot was taken.
    stamped_note(&store, &dir, "saved-since", 0);
    let conn = second_connection(&dir);
    legacy_snapshot(
        &conn,
        "heartbeat",
        r#"{"buffers":{"saved-since":"older text"}}"#,
        -30,
        false,
    );

    let carried = store.carry_over_legacy_snapshots().expect("carry over");
    assert_eq!(carried.journaled, 0);
    assert_eq!(carried.emptied, 1);
    assert_eq!(store.unsaved_text_count().expect("count"), 0);
    assert_eq!(snapshot_state(&conn, "heartbeat"), "{}");
}

#[test]
fn a_clean_or_superseded_snapshot_of_0_5_is_emptied_and_journals_nothing() {
    let (dir, store) = setup_store();
    stamped_note(&store, &dir, "note-1", -60);
    let conn = second_connection(&dir);
    legacy_snapshot(
        &conn,
        "older-unclean",
        r#"{"buffers":{"note-1":"older"}}"#,
        -30,
        false,
    );
    legacy_snapshot(
        &conn,
        "latest-clean",
        r#"{"buffers":{"note-1":"on disk"}}"#,
        0,
        true,
    );

    let carried = store.carry_over_legacy_snapshots().expect("carry over");
    assert_eq!(carried.journaled, 0);
    assert_eq!(carried.emptied, 2);
    assert_eq!(snapshot_state(&conn, "older-unclean"), "{}");
    assert_eq!(snapshot_state(&conn, "latest-clean"), "{}");
}

#[test]
fn an_undecodable_snapshot_of_0_5_is_reported_kept_and_never_pruned() {
    let (dir, mut store) = setup_store();
    stamped_note(&store, &dir, "note-1", -60);
    let conn = second_connection(&dir);
    let undecodable = r#"{"buffers":{"note-1":42}}"#;
    legacy_snapshot(&conn, "legacy", undecodable, -10, false);

    let carried = store.carry_over_legacy_snapshots().expect("carry over");
    assert_eq!(carried.undecodable, 1);
    assert_eq!(carried.journaled, 0);
    assert_eq!(carried.emptied, 0);
    assert_eq!(snapshot_state(&conn, "legacy"), undecodable);

    // Session markers keep arriving; the row they would prune is still there.
    store.mark_session_running().expect("running");
    for _ in 0..8 {
        store.mark_session_clean().expect("clean");
    }
    assert_eq!(snapshot_state(&conn, "legacy"), undecodable);
    assert_eq!(snapshot_count(&conn), 6, "five markers and the kept row");
}

#[test]
fn the_carry_over_does_nothing_the_second_time() {
    let (dir, store) = setup_store();
    stamped_note(&store, &dir, "kept", -30);
    let conn = second_connection(&dir);
    legacy_snapshot(
        &conn,
        "shutdown",
        r#"{"buffers":{"kept":"typed"}}"#,
        0,
        false,
    );

    store.carry_over_legacy_snapshots().expect("first launch");
    store.clear_unsaved_text("kept").expect("restored");
    let again = store.carry_over_legacy_snapshots().expect("second launch");
    assert_eq!(again.journaled, 0);
    assert_eq!(again.emptied, 0);
    assert_eq!(store.unsaved_text_count().expect("count"), 0);
}

#[test]
fn resolve_recovery_snapshot_newer_returns_restore() {
    let resolution = resolve_recovery("2024-01-01 12:00:01", "2024-01-01 12:00:00");
    assert_eq!(resolution, RecoveryResolution::Restore);
}

#[test]
fn resolve_recovery_snapshot_older_returns_ignore() {
    let resolution = resolve_recovery("2024-01-01 11:59:59", "2024-01-01 12:00:00");
    assert_eq!(resolution, RecoveryResolution::Ignore);
}

#[test]
fn resolve_recovery_same_timestamp_returns_ignore() {
    let resolution = resolve_recovery("2024-01-01 12:00:00", "2024-01-01 12:00:00");
    assert_eq!(resolution, RecoveryResolution::Ignore);
}
