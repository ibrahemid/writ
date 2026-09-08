//! The one guarded write, asserted on its own.
//!
//! Every writer in the crate reaches a note's file through
//! `writ_storage::guarded`, so what that module decides is what the editor,
//! the command line, a rename propagation and a relaunch all do. The rules
//! are ADR-028 §5's: a file that changed under Writ is refused with the
//! incoming text set aside beside it, a file that already holds the incoming
//! text is left alone, and a file whose bytes are not on this machine is
//! refused before the read that would pull it down.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_core::hash::sha256_bytes;
use writ_core::notes::guard::{DiskState, SaveDecision, SF_DATALESS};
use writ_core::notes::WriteOrigin;
use writ_storage::errors::StorageError;
use writ_storage::guarded::{
    create_note_guarded, guard_rename, write_note_guarded, ConflictPolicy, CreateNote, DiskRead,
    GuardedWrite, WriteCapture,
};

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

/// A note holding `text`, written into a fresh folder.
fn seeded(text: &str) -> (TempDir, PathBuf) {
    let root = TempDir::new().expect("temp dir");
    let path = root.path().join("Note.md");
    std::fs::write(&path, text).expect("seed");
    (root, path)
}

/// An ordinary save of `bytes` over `path`, with everything else at its
/// default.
fn saving<'a>(path: &'a Path, bytes: &'a [u8], last_known: Option<DiskState>) -> GuardedWrite<'a> {
    GuardedWrite {
        target: path,
        bytes,
        last_known,
        on_disk: DiskRead::Fresh,
        dataless: None,
        origin: WriteOrigin::Editor,
        on_conflict: ConflictPolicy::RefuseWithCopy,
        history: None,
    }
}

/// One write, as the version store saw it.
struct Captured {
    target: PathBuf,
    before: Option<Vec<u8>>,
    after: DiskState,
}

/// What the writes handed the version store, in order.
#[derive(Default)]
struct Captures {
    seen: RefCell<Vec<Captured>>,
}

impl Captures {
    fn hook(&self) -> impl Fn(WriteCapture<'_>) + '_ {
        move |capture: WriteCapture<'_>| {
            self.seen.borrow_mut().push(Captured {
                target: capture.target.to_path_buf(),
                before: capture.before.map(<[u8]>::to_vec),
                after: *capture.after,
            });
        }
    }

    fn count(&self) -> usize {
        self.seen.borrow().len()
    }
}

#[test]
fn a_file_nothing_touched_is_written_and_the_guard_proceeds() {
    let (_root, path) = seeded("the first text\n");
    let last_known = recorded(&path);

    let outcome = write_note_guarded(saving(&path, b"the second text\n", Some(last_known)), None)
        .expect("the guard should let this through");

    assert_eq!(outcome.decision, SaveDecision::Proceed);
    assert_eq!(outcome.conflict_copy, None);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "the second text\n"
    );
    assert_eq!(outcome.disk_state.hash, sha256_bytes(b"the second text\n"));
}

#[test]
fn a_file_that_already_holds_the_incoming_text_is_left_alone_without_a_word() {
    // Somebody else landed the same edit first. Writ has a stale record and
    // the file differs from it, but it holds exactly what is being written, so
    // there is nothing to warn about and nothing to write: rewriting identical
    // bytes moves the modification time and swaps the inode for a change the
    // user cannot see, which a sync client then uploads.
    let (_root, path) = seeded("what Writ last read\n");
    let last_known = recorded(&path);
    std::fs::write(&path, "the same edit, from elsewhere\n").expect("write");
    let before = std::fs::metadata(&path).expect("metadata");

    let outcome = write_note_guarded(
        saving(&path, b"the same edit, from elsewhere\n", Some(last_known)),
        None,
    )
    .expect("identical text is not a conflict");

    assert_eq!(outcome.decision, SaveDecision::AlreadyIdentical);
    assert_eq!(outcome.conflict_copy, None);
    assert_eq!(
        std::fs::metadata(&path).expect("metadata").modified().ok(),
        before.modified().ok(),
        "the file was rewritten with the bytes it already held"
    );
    assert!(
        siblings(&path).is_empty(),
        "a copy was written beside a note nothing happened to"
    );
}

#[test]
fn a_file_changed_underneath_is_refused_and_the_incoming_text_lands_beside_it() {
    let (_root, path) = seeded("what Writ last read\n");
    let last_known = recorded(&path);
    std::fs::write(&path, "what somebody else wrote\n").expect("write");

    let error = write_note_guarded(
        saving(&path, b"what the tab holds\n", Some(last_known)),
        None,
    )
    .expect_err("a file changed underneath should be refused");

    let StorageError::SourceChangedOnDisk {
        path: named,
        conflict_copy,
        ..
    } = error
    else {
        panic!("expected a changed file, got {error:?}");
    };
    assert_eq!(named, path.to_string_lossy());
    let copy = conflict_copy.expect("a refusal never ends with the text nowhere");
    assert_eq!(
        std::fs::read_to_string(&copy).expect("read"),
        "what the tab holds\n"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "what somebody else wrote\n",
        "the refused write landed anyway"
    );
}

#[test]
fn a_file_whose_time_moved_but_whose_text_did_not_raises_no_conflict() {
    // A touch, a sync round trip and a Time Machine restore all move the
    // modification time without changing a byte. Only the digests decide, so
    // the record is built with a timestamp the file cannot have rather than
    // by racing the filesystem clock.
    let (_root, path) = seeded("the text\n");
    let last_known = DiskState {
        mtime: Some(std::time::UNIX_EPOCH),
        ..recorded(&path)
    };

    let outcome = write_note_guarded(saving(&path, b"the next text\n", Some(last_known)), None)
        .expect("a moved timestamp is not a change");

    assert_eq!(outcome.decision, SaveDecision::Proceed);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "the next text\n"
    );
}

#[cfg(unix)]
#[test]
fn a_read_only_destination_is_refused_and_left_as_it_was() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, path) = seeded("the text\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");

    let error = write_note_guarded(saving(&path, b"the next text\n", None), None)
        .expect_err("a read-only file should be refused");

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let StorageError::DestinationReadOnly { path: named } = error else {
        panic!("expected a read-only destination, got {error:?}");
    };
    assert_eq!(named, path.display().to_string());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "the text\n");
}

#[test]
fn a_file_that_is_not_downloaded_is_refused_before_it_is_read() {
    // The probe stands in for `SF_DATALESS`, so the refusal is asserted on
    // every platform rather than only where the flag can be set. Reading an
    // evicted file is what makes the provider daemon fetch it (ADR-028 §5),
    // and the read the guard would do is the compare read.
    let (_root, path) = seeded("the text\n");
    let probe = |_: &Path| Some(SF_DATALESS);
    let mut request = saving(&path, b"the next text\n", Some(recorded(&path)));
    request.dataless = Some(&probe);

    let error = write_note_guarded(request, None).expect_err("an evicted file should be refused");

    let StorageError::SourceNotDownloaded { path: named } = error else {
        panic!("expected a file that is not downloaded, got {error:?}");
    };
    assert_eq!(named, path.to_string_lossy());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "the text\n");
    assert!(
        siblings(&path).is_empty(),
        "a copy was written beside a file the guard never opened"
    );
}

#[cfg(unix)]
#[test]
fn a_new_note_whose_name_is_already_on_disk_is_refused_without_truncating_it() {
    // The dedupe learns which names are taken by listing the folder, and a
    // folder it cannot list reads as empty. The write that follows replaces
    // whatever is at the path, so minting checks the name it picked against
    // the disk before writing.
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new().expect("temp dir");
    let taken = root.path().join("Notes.md");
    std::fs::write(&taken, "what was already there\n").expect("seed");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o300)).expect("chmod");

    let outcome = create_note_guarded(
        CreateNote {
            notes_root: root.path(),
            stem: "Notes",
            content: "the new note\n",
            origin: WriteOrigin::Editor,
            history: None,
        },
        None,
    );

    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let Err(StorageError::NoteNameTaken { name, .. }) = outcome else {
        panic!("expected a taken name, got {outcome:?}");
    };
    assert_eq!(name, "Notes.md");
    assert_eq!(
        std::fs::read_to_string(&taken).expect("read"),
        "what was already there\n",
        "the note that was already there was written over"
    );
}

#[test]
fn a_new_note_is_minted_under_the_origin_that_asked_for_it() {
    let root = TempDir::new().expect("temp dir");
    let captures = Captures::default();
    let hook = captures.hook();

    let path = create_note_guarded(
        CreateNote {
            notes_root: root.path(),
            stem: "Notes",
            content: "the new note\n",
            origin: WriteOrigin::Cli,
            history: Some(&hook),
        },
        None,
    )
    .expect("create");

    assert_eq!(path, root.path().join("Notes.md"));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "the new note\n"
    );
    let seen = captures.seen.borrow();
    let captured = seen.first().expect("the write was captured");
    assert_eq!(captured.target, path);
    assert_eq!(
        captured.before, None,
        "a note that did not exist had nothing to capture"
    );
    assert_eq!(captured.after.hash, sha256_bytes(b"the new note\n"));
}

#[test]
fn a_write_that_landed_is_captured_once_with_what_the_file_held_before_it() {
    let (_root, path) = seeded("what was there\n");
    let captures = Captures::default();
    let hook = captures.hook();
    let mut request = saving(&path, b"what is there now\n", Some(recorded(&path)));
    request.history = Some(&hook);

    write_note_guarded(request, None).expect("write");

    assert_eq!(captures.count(), 1);
    let seen = captures.seen.borrow();
    let captured = &seen[0];
    assert_eq!(captured.target, path);
    assert_eq!(
        captured.before.as_deref(),
        Some(b"what was there\n".as_slice())
    );
    assert_eq!(captured.after.hash, sha256_bytes(b"what is there now\n"));
}

#[test]
fn a_write_that_never_landed_is_never_captured() {
    // Two ways a write does not land: the guard refused it, and the file
    // already held the text. Neither is a version of anything.
    let (_root, refused) = seeded("what Writ last read\n");
    let last_known = recorded(&refused);
    std::fs::write(&refused, "what somebody else wrote\n").expect("write");
    let captures = Captures::default();
    let hook = captures.hook();

    let mut request = saving(&refused, b"what the tab holds\n", Some(last_known));
    request.history = Some(&hook);
    write_note_guarded(request, None).expect_err("refused");
    assert_eq!(captures.count(), 0);

    let (_root, identical) = seeded("the text\n");
    let mut request = saving(&identical, b"the text\n", Some(recorded(&identical)));
    request.history = Some(&hook);
    write_note_guarded(request, None).expect("identical text is not a conflict");
    assert_eq!(captures.count(), 0);
}

#[test]
fn a_rename_of_a_file_changed_underneath_is_refused_with_nothing_set_aside() {
    // A rename carries no text of its own, so there is nothing to write
    // beside the note.
    let (_root, path) = seeded("what Writ last read\n");
    let last_known = recorded(&path);
    std::fs::write(&path, "what somebody else wrote\n").expect("write");

    let error = guard_rename(&path, Some(last_known), WriteOrigin::Editor)
        .expect_err("a file changed underneath should be refused");

    let StorageError::SourceChangedOnDisk { conflict_copy, .. } = error else {
        panic!("expected a changed file, got {error:?}");
    };
    assert_eq!(conflict_copy, None);
    assert!(siblings(&path).is_empty());
}

/// Every file in the note's folder except the note itself.
fn siblings(note: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(note.parent().expect("a note sits in a folder"))
        .expect("read dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path != note)
        .collect()
}
