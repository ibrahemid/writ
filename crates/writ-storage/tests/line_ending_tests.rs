//! A save writes the line ending the file already had.
//!
//! The editor normalises to LF on load, so without this every save of a
//! Windows note rewrites every line of it and a sync client uploads a file
//! whose only change is invisible.

use std::path::Path;

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::hash::sha256_bytes;
use writ_core::notes::guard::DiskState;
use writ_core::notes::line_ending::LineEnding;
use writ_storage::buffer_store::{BufferStore, RecoveredText};
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    (dir, BufferStore::new(conn, buffers_dir))
}

fn recorded(path: &Path) -> DiskState {
    let bytes = std::fs::read(path).expect("read");
    let metadata = std::fs::metadata(path).expect("metadata");
    DiskState {
        hash: sha256_bytes(&bytes),
        size: metadata.len(),
        mtime: metadata.modified().ok(),
    }
}

/// Opens a note on a file already holding `on_disk`, the way the open path
/// does: the ending is read off the bytes and carried on the row.
fn open_note(store: &BufferStore, dir: &TempDir, on_disk: &str) -> (std::path::PathBuf, DiskState) {
    let path = dir.path().join("notes.md");
    std::fs::write(&path, on_disk).expect("write");
    let now = Utc::now();
    let doc = BufferDocument {
        id: "ending-1".to_string(),
        title: "notes".to_string(),
        filename: "ending-1.txt".to_string(),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(path.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: on_disk.len() as u64,
        line_ending: LineEnding::detect(on_disk),
    };
    store.open_from_path(&doc, on_disk).expect("open");
    let state = recorded(&path);
    (path, state)
}

/// The file's identity, so a test can prove no replacement happened.
#[cfg(unix)]
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).expect("metadata").ino()
}

/// Windows has no inode; a save that was refused leaves a dated copy instead,
/// which the same tests assert on either platform.
#[cfg(not(unix))]
fn inode(_path: &Path) -> u64 {
    0
}

/// [`inode`] from metadata already taken, for a before-and-after comparison.
#[cfg(unix)]
fn inode_of(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(not(unix))]
fn inode_of(_metadata: &std::fs::Metadata) -> u64 {
    0
}

fn conflict_copies(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("(conflict "))
        .collect()
}

#[test]
fn a_windows_file_is_saved_back_with_its_carriage_returns() {
    let (dir, store) = setup();
    let (path, state) = open_note(&store, &dir, "alpha\r\nbeta\r\ngamma\r\n");

    // What CodeMirror hands back: the same text with one line edited, LF only.
    store
        .save_to_source("ending-1", "alpha\nBETA\ngamma\n", Some(state), None)
        .expect("save");

    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"alpha\r\nBETA\r\ngamma\r\n"
    );
}

#[test]
fn a_unix_file_gains_no_carriage_returns() {
    let (dir, store) = setup();
    let (path, state) = open_note(&store, &dir, "alpha\nbeta\n");

    store
        .save_to_source("ending-1", "alpha\nBETA\n", Some(state), None)
        .expect("save");

    assert_eq!(std::fs::read(&path).expect("read"), b"alpha\nBETA\n");
}

#[test]
fn a_mixed_file_is_saved_in_its_dominant_ending() {
    let (dir, store) = setup();
    // Three CRLF breaks against one LF: the file is a CRLF file with a stray
    // line in it, and a save settles the whole file on the majority.
    let (path, state) = open_note(&store, &dir, "a\r\nb\r\nc\nd\r\n");

    store
        .save_to_source("ending-1", "a\nb\nc\nd\n", Some(state), None)
        .expect("save");

    assert_eq!(std::fs::read(&path).expect("read"), b"a\r\nb\r\nc\r\nd\r\n");
}

#[test]
fn a_windows_file_already_holding_the_incoming_text_is_left_alone() {
    let (dir, store) = setup();
    let (path, stale) = open_note(&store, &dir, "alpha\r\nbeta\r\n");

    // Somebody else — a sync client, another editor — landed the same edit
    // first, in the file's own convention. Writ's record is now stale.
    std::fs::write(&path, "alpha\r\nBETA\r\n").expect("external write");
    let untouched = inode(&path);

    // The guard compares the digest of the bytes that would land, so the
    // editor's LF text has to be turned back into CRLF before it is hashed.
    // Hashing the LF form instead would see a difference that is not there
    // and refuse the save, leaving a dated copy beside the note.
    let after = store
        .save_to_source("ending-1", "alpha\nBETA\n", Some(stale), None)
        .expect("save");

    assert_eq!(after.hash, sha256_bytes(b"alpha\r\nBETA\r\n"));
    assert_eq!(std::fs::read(&path).expect("read"), b"alpha\r\nBETA\r\n");
    assert_eq!(untouched, inode(&path), "the file was rewritten");
    assert!(
        conflict_copies(dir.path()).is_empty(),
        "the save was refused"
    );
}

#[test]
fn a_note_writ_creates_is_saved_in_lf() {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("Writ");
    // A file that does not exist yet has no convention to keep, so the copy
    // lands in LF whatever the text handed in carries.
    let path =
        writ_storage::note_ops::save_copy(&notes, "Fresh", "one\r\ntwo\r\n", None).expect("copy");

    assert_eq!(std::fs::read(&path).expect("read"), b"one\ntwo\n");
}

#[test]
fn a_mixed_file_the_crash_did_not_change_is_left_alone() {
    let (dir, store) = setup();
    // Three CRLF breaks and one LF, so the note's recorded ending is CRLF
    // while the file itself is mixed.
    let mixed = "a\r\nb\r\nc\nd\r\n";
    let (path, _state) = open_note(&store, &dir, mixed);
    let before = inode(&path);

    // The crash snapshot is the file read byte for byte, so it comes back
    // holding the same mixed text. Encoding it to the note's ending here
    // would settle the stray line, and the file would then read as having
    // moved on while Writ was down.
    let outcome = store
        .restore_recovered_content("ending-1", mixed, None, None)
        .expect("restore");

    assert!(
        matches!(outcome, RecoveredText::Restored(_)),
        "the file was set aside: {outcome:?}"
    );
    assert_eq!(std::fs::read(&path).expect("read"), mixed.as_bytes());
    assert_eq!(before, inode(&path), "the file was replaced");
    assert!(
        conflict_copies(dir.path()).is_empty(),
        "a copy was written beside the note"
    );
    assert!(
        std::fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(Result::ok)
            .all(|e| !e.file_name().to_string_lossy().contains("(recovered ")),
        "a recovered copy was written beside the note"
    );
}

#[test]
fn a_save_of_the_text_the_file_already_holds_leaves_it_alone() {
    let (dir, store) = setup();
    let (path, state) = open_note(&store, &dir, "alpha\r\nbeta\r\n");
    let before = std::fs::metadata(&path).expect("stat");

    // Cmd+S with nothing typed: the editor hands back what it loaded, which
    // encodes to exactly the bytes already there.
    store
        .save_to_source("ending-1", "alpha\nbeta\n", Some(state), None)
        .expect("save");

    let after = std::fs::metadata(&path).expect("stat");
    assert_eq!(std::fs::read(&path).expect("read"), b"alpha\r\nbeta\r\n");
    assert_eq!(inode(&path), inode_of(&before), "the file was replaced");
    assert_eq!(
        after.modified().ok(),
        before.modified().ok(),
        "nothing was written, so the modification time cannot have moved"
    );
}

#[test]
fn the_copy_beside_a_refused_save_carries_the_files_ending() {
    let (dir, store) = setup();
    let (path, stale) = open_note(&store, &dir, "alpha\r\nbeta\r\n");

    // Something else rewrote the file while the tab was open, so the save is
    // refused and the user's text is written beside it instead.
    std::fs::write(&path, "alpha\r\nsomebody else\r\n").expect("external write");

    let refusal = store
        .save_to_source("ending-1", "alpha\nwhat the user typed\n", Some(stale), None)
        .expect_err("the save must be refused");

    let copies = conflict_copies(dir.path());
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(
        format!("{refusal}").contains("changed on disk"),
        "{refusal:?}"
    );
    // The copy is what the user is told to look at, so it has to be readable
    // in the same place the note came from rather than in the editor's LF.
    assert_eq!(
        std::fs::read(dir.path().join(&copies[0])).expect("read copy"),
        b"alpha\r\nwhat the user typed\r\n"
    );
}

#[test]
fn the_recorded_ending_survives_being_set() {
    let (dir, store) = setup();
    let (_path, _state) = open_note(&store, &dir, "alpha\nbeta\n");
    assert_eq!(
        store.get("ending-1").expect("get").line_ending,
        LineEnding::Lf
    );

    store
        .set_line_ending("ending-1", LineEnding::CrLf)
        .expect("set");

    assert_eq!(
        store.get("ending-1").expect("get").line_ending,
        LineEnding::CrLf,
        "the ending was not written back to the row"
    );
    // A row read fresh from the database, so the column is proved and not
    // just the value the store happened to be holding.
    let reopened = BufferStore::new(
        open_database(&dir.path().join("test.db")).expect("open db"),
        dir.path().join("buffers"),
    );
    assert_eq!(
        reopened.get("ending-1").expect("get").line_ending,
        LineEnding::CrLf,
        "the ending did not survive a restart"
    );
}
