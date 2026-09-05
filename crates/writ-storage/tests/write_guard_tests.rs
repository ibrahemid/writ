//! The write guard at the layer that touches the disk (ADR-028 §5).

use std::path::Path;

use chrono::{DateTime, Utc};
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::hash::{sha256_bytes, sha256_hex};
use writ_core::notes::guard::{DiskState, SF_DATALESS};
use writ_storage::buffer_store::{write_conflict_copy, BufferStore, RecoveredText};
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    (dir, BufferStore::new(conn, buffers_dir))
}

fn make_doc(id: &str, source_path: &Path) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: "notes".to_string(),
        filename: format!("{id}.md"),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(source_path.to_string_lossy().into_owned()),
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

/// What the file holds right now, as the adapter records it after a read.
fn recorded(path: &Path) -> DiskState {
    let bytes = std::fs::read(path).expect("read");
    let metadata = std::fs::metadata(path).expect("metadata");
    DiskState {
        hash: sha256_bytes(&bytes),
        size: metadata.len(),
        mtime: metadata.modified().ok(),
    }
}

/// A note opened from a file holding `content`, with what Writ recorded then.
fn open_note(store: &BufferStore, dir: &TempDir, content: &str) -> (std::path::PathBuf, DiskState) {
    let path = dir.path().join("notes.md");
    std::fs::write(&path, content).expect("write");
    let doc = make_doc("guard-1", &path);
    store.open_from_path(&doc, content).expect("open");
    let state = recorded(&path);
    (path, state)
}

fn conflict_copies(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("(conflict "))
        .collect();
    names.sort();
    names
}

fn recovered_copies(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("(recovered "))
        .collect();
    names.sort();
    names
}

fn fixed_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-29T09:41:07Z")
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn save_after_an_out_of_band_write_refuses_with_source_changed_on_disk_and_leaves_the_bytes_unchanged(
) {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let (path, last_known) = open_note(&store, &notes, "# What Writ read");

    std::fs::write(&path, "# What another program wrote").unwrap();

    let result = store.save_to_source("guard-1", "# What the user typed", Some(last_known), None);

    match result {
        Err(StorageError::SourceChangedOnDisk {
            path: refused_path,
            disk_hash,
            ..
        }) => {
            assert_eq!(refused_path, path.to_string_lossy());
            assert_eq!(
                disk_hash,
                sha256_hex(b"# What another program wrote"),
                "the digest names the bytes that are actually there"
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# What another program wrote",
        "the change on disk survives the refusal"
    );
}

#[test]
fn refused_save_writes_a_dated_conflict_copy_beside_the_note() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let (path, last_known) = open_note(&store, &notes, "# What Writ read");
    std::fs::write(&path, "# What another program wrote").unwrap();

    let result = store.save_to_source("guard-1", "# What the user typed", Some(last_known), None);
    assert!(result.is_err());

    let copies = conflict_copies(notes.path());
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(copies[0].starts_with("notes (conflict "), "{:?}", copies[0]);
    assert!(copies[0].ends_with(").md"), "{:?}", copies[0]);
    assert_eq!(
        std::fs::read_to_string(notes.path().join(&copies[0])).unwrap(),
        "# What the user typed",
        "the copy holds the side that was about to be lost"
    );
}

#[test]
fn a_refused_save_names_the_conflict_copy_in_the_error() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let (path, last_known) = open_note(&store, &notes, "# What Writ read");
    std::fs::write(&path, "# What another program wrote").unwrap();

    let result = store.save_to_source("guard-1", "# What the user typed", Some(last_known), None);

    let StorageError::SourceChangedOnDisk { conflict_copy, .. } = result.unwrap_err() else {
        panic!("expected a refusal");
    };
    let named = conflict_copy.expect("the refusal names where the text went");
    assert!(Path::new(&named).is_file(), "{named}");
    assert_eq!(
        std::fs::read_to_string(&named).unwrap(),
        "# What the user typed"
    );
}

#[test]
fn identical_content_on_disk_saves_silently_with_no_error() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let (path, last_known) = open_note(&store, &notes, "# What Writ read");

    std::fs::write(&path, "# The same edit, made twice").unwrap();
    let before = recorded(&path);

    let after = store
        .save_to_source(
            "guard-1",
            "# The same edit, made twice",
            Some(last_known),
            None,
        )
        .expect("an identical write is not a conflict");

    assert_eq!(after, before, "nothing was written, so nothing moved");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# The same edit, made twice"
    );
    assert!(conflict_copies(notes.path()).is_empty());
}

#[test]
fn touching_the_file_without_changing_it_does_not_refuse() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let (path, last_known) = open_note(&store, &notes, "# What Writ read");

    // A touch, a sync round trip and a restore all rewrite the same bytes.
    std::fs::write(&path, "# What Writ read").unwrap();

    let after = store
        .save_to_source("guard-1", "# What the user typed", Some(last_known), None)
        .expect("mtime is never the signal");

    assert_eq!(after.hash, sha256_bytes(b"# What the user typed"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# What the user typed"
    );
    assert!(conflict_copies(notes.path()).is_empty());
}

#[test]
fn conflict_copy_name_matches_the_dated_pattern() {
    let notes = TempDir::new().unwrap();
    let path = notes.path().join("Meeting notes.md");
    std::fs::write(&path, "on disk").unwrap();

    let written = write_conflict_copy(&path, "mine", fixed_now(), None).expect("copy");

    let name = written.file_name().unwrap().to_string_lossy().into_owned();
    let stamp = fixed_now()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H.%M.%S");
    assert_eq!(name, format!("Meeting notes (conflict {stamp}).md"));
    assert_eq!(std::fs::read_to_string(&written).unwrap(), "mine");
}

#[test]
fn the_conflict_copy_dedupes_when_one_exists() {
    let notes = TempDir::new().unwrap();
    let path = notes.path().join("Meeting notes.md");
    std::fs::write(&path, "on disk").unwrap();

    let first = write_conflict_copy(&path, "first", fixed_now(), None).expect("first copy");
    let second = write_conflict_copy(&path, "second", fixed_now(), None).expect("second copy");

    assert_ne!(first, second, "the second copy never lands on the first");
    let stamp = fixed_now()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H.%M.%S");
    assert_eq!(
        second.file_name().unwrap().to_string_lossy(),
        format!("Meeting notes (conflict {stamp}) 2.md")
    );
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "first");
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "second");
}

/// A note whose text the last session never flushed: the row and the file
/// exist, the snapshot holds text, and nothing recorded what the file held.
fn open_recovered_note(store: &BufferStore, dir: &TempDir, on_disk: &str) -> std::path::PathBuf {
    let path = dir.path().join("notes.md");
    std::fs::write(&path, on_disk).expect("write");
    let doc = make_doc("guard-1", &path);
    store.open_from_path(&doc, on_disk).expect("open");
    path
}

#[test]
fn a_file_changed_while_writ_was_down_keeps_its_text_and_the_snapshot_lands_beside_it() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let path = open_recovered_note(&store, &notes, "# What a sync client delivered");

    let outcome = store
        .restore_recovered_content("guard-1", "# What the crash was holding", None, None)
        .expect("recovery never fails on a file that moved on");

    let RecoveredText::SetAside { on_disk, copy } = outcome else {
        panic!("expected the snapshot to be set aside, got {outcome:?}");
    };
    let on_disk = on_disk.expect("the file was read, so what it holds is known");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# What a sync client delivered",
        "the newer version survives the relaunch"
    );
    assert_eq!(
        on_disk.hash,
        sha256_bytes(b"# What a sync client delivered")
    );
    assert_eq!(
        std::fs::read_to_string(&copy).unwrap(),
        "# What the crash was holding"
    );
    let name = copy.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("notes (recovered "), "{name}");
    assert!(name.ends_with(").md"), "{name}");
}

#[test]
fn a_file_that_did_not_change_is_left_exactly_as_it_is() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let path = open_recovered_note(&store, &notes, "# What the crash was holding");
    let before = std::fs::metadata(&path).unwrap();

    let outcome = store
        .restore_recovered_content("guard-1", "# What the crash was holding", None, None)
        .expect("restore");

    let RecoveredText::Restored(state) = outcome else {
        panic!("the file holds the recovered text, so it is restored: {outcome:?}");
    };
    assert_eq!(state.hash, sha256_bytes(b"# What the crash was holding"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# What the crash was holding"
    );
    assert!(conflict_copies(notes.path()).is_empty());
    assert!(recovered_copies(notes.path()).is_empty());

    // Rewriting identical bytes would move the modification time and, because
    // the write is a rename into place, swap the inode. A sync client reads
    // either as an edit and uploads the file again.
    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(
        after.modified().unwrap(),
        before.modified().unwrap(),
        "nothing was written, so the modification time cannot have moved"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(after.ino(), before.ino(), "the file was never replaced");
    }
}

// The only way to reach this branch: a note typed into and never saved, whose
// path is minted by the relaunch a moment before this runs. Nothing has ever
// been at it, so writing it creates the note rather than putting back a
// deletion. A note whose file was deleted never gets here at all; that is
// `plan_recovery`'s call, pinned in `writ-core` and over the real relaunch in
// `src-tauri/tests/recovery_startup_tests.rs`.
#[test]
fn a_note_that_never_reached_a_file_is_written_at_its_own_path() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let path = notes.path().join("notes.md");
    let doc = make_doc("guard-1", &path);
    store.insert(&doc).expect("insert");

    let outcome = store
        .restore_recovered_content("guard-1", "# What the crash was holding", None, None)
        .expect("restore");

    match outcome {
        RecoveredText::Restored(state) => {
            assert_eq!(state.hash, sha256_bytes(b"# What the crash was holding"));
        }
        other => panic!("expected the note to be written, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# What the crash was holding"
    );
    assert!(recovered_copies(notes.path()).is_empty());
}

#[test]
fn every_write_is_announced_before_it_lands() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let (path, last_known) = open_note(&store, &notes, "# What Writ read");
    std::fs::write(&path, "# What another program wrote").unwrap();

    let announced = std::cell::RefCell::new(Vec::<(std::path::PathBuf, bool)>::new());
    let stamp = |target: &Path, bytes: &[u8]| {
        // The stamp has to be in place before the bytes are, or the folder's
        // watcher reads Writ's own write as somebody else's edit.
        announced
            .borrow_mut()
            .push((target.to_path_buf(), target.exists()));
        assert_eq!(bytes, b"# What the user typed");
    };

    let result = store.save_to_source(
        "guard-1",
        "# What the user typed",
        Some(last_known),
        Some(&stamp),
    );
    assert!(result.is_err());

    let announced = announced.into_inner();
    assert_eq!(announced.len(), 1, "{announced:?}");
    assert!(
        announced[0]
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("(conflict "),
        "the copy a stopped save writes is announced too: {announced:?}"
    );
    assert!(!announced[0].1, "announced before the file existed");
    assert!(announced[0].0.is_file(), "and it exists afterwards");
}

#[test]
fn an_evicted_file_is_never_read_and_the_snapshot_lands_beside_it() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let path = open_recovered_note(&store, &notes, "# Only a placeholder is here");

    // The flag cannot be set on a test machine, so it is injected. Reading an
    // evicted file is what makes the provider daemon fetch it, and a relaunch
    // is the worst moment to download every note.
    let evicted = |_: &Path| Some(SF_DATALESS);

    let outcome = store
        .restore_recovered_content(
            "guard-1",
            "# What the crash was holding",
            None,
            Some(&evicted),
        )
        .expect("an evicted file is not a failure");

    let RecoveredText::SetAside { on_disk, copy } = outcome else {
        panic!("expected the snapshot to be set aside, got {outcome:?}");
    };
    assert!(
        on_disk.is_none(),
        "nothing was read, so nothing can be claimed about the file"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# Only a placeholder is here",
        "the file is left exactly as it was"
    );
    assert_eq!(
        std::fs::read_to_string(&copy).unwrap(),
        "# What the crash was holding"
    );
    assert!(
        copy.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("notes (recovered "),
        "{copy:?}"
    );
}

#[test]
fn a_file_the_flags_call_downloaded_takes_the_ordinary_route() {
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let path = open_recovered_note(&store, &notes, "# What the crash was holding");
    let downloaded = |_: &Path| Some(0u32);

    let outcome = store
        .restore_recovered_content(
            "guard-1",
            "# What the crash was holding",
            None,
            Some(&downloaded),
        )
        .expect("restore");

    assert!(matches!(outcome, RecoveredText::Restored(_)));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# What the crash was holding"
    );
    assert!(recovered_copies(notes.path()).is_empty());
}

#[test]
fn keeping_mine_over_a_guard_that_refuses_still_leaves_both_texts_on_disk() {
    // The order a resolved change runs in: the file's text is written beside
    // the note first, and the document is only written over it afterwards. A
    // file that changed again in between makes the guard refuse the second
    // half, and both texts are still on disk when it does — the first copy
    // holds what the file held, and the guard's own copy holds what was being
    // written.
    let (_db, store) = setup();
    let notes = TempDir::new().unwrap();
    let (path, stale) = open_note(&store, &notes, "# What Writ read");
    std::fs::write(&path, "# What another program wrote").unwrap();

    let copy = write_conflict_copy(&path, "# What another program wrote", fixed_now(), None)
        .expect("the file's text is written beside the note first");

    let result = store.save_to_source("guard-1", "# What the user typed", Some(stale), None);
    assert!(
        matches!(result, Err(StorageError::SourceChangedOnDisk { .. })),
        "{result:?}"
    );

    assert_eq!(
        std::fs::read_to_string(&copy).unwrap(),
        "# What another program wrote"
    );
    let copies = conflict_copies(notes.path());
    assert_eq!(copies.len(), 2, "{copies:?}");
    let texts: Vec<String> = copies
        .iter()
        .map(|name| std::fs::read_to_string(notes.path().join(name)).unwrap())
        .collect();
    assert!(texts.contains(&"# What another program wrote".to_string()));
    assert!(texts.contains(&"# What the user typed".to_string()));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# What another program wrote",
        "the refusal left the file alone"
    );
}
