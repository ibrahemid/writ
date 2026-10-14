//! The write lock two Writ processes hold around a guarded write's last read
//! and its rename (AM-40, ADR-031 rule 1.4).
//!
//! A second thread stands in for the second process: the lock belongs to an
//! open file, so two threads holding two opens exclude each other the way two
//! processes do. The race is staged with the save's own stamp hook, which runs
//! after its last read and before its rename: the other writer is let go there
//! and given time to land.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use tempfile::TempDir;
use writ_core::config::FileExtension;
use writ_core::notes::host::{Capability, LastKnown, NoteHost};
use writ_core::notes::WriteOrigin;
use writ_storage::buffer_store::read_disk_state;
use writ_storage::errors::StorageError;
use writ_storage::guarded::{write_note_guarded, ConflictPolicy, DiskRead, GuardedWrite};
use writ_storage::note_host::NoteHostImpl;
use writ_storage::write_lock::{WriteLock, WRITE_LOCK_FILE};

/// How long the save holds its rename back for the other writer.
const STAGE: Duration = Duration::from_millis(400);

const READ_BY_BOTH: &str = "the text both read\n";
const APP_TEXT: &str = "the app's text\n";
const PROGRAM_TEXT: &str = "the program's text\n";

struct Fixture {
    _dir: TempDir,
    data: PathBuf,
    notes: PathBuf,
    note: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes folder");
    let note = notes.join("Launch.md");
    std::fs::write(&note, READ_BY_BOTH).expect("seed the note");
    Fixture {
        data: dir.path().to_path_buf(),
        notes,
        note,
        _dir: dir,
    }
}

/// Every text the notes folder holds, sorted.
fn texts_in(notes: &Path) -> Vec<String> {
    let mut texts: Vec<String> = std::fs::read_dir(notes)
        .expect("read the folder")
        .map(|entry| std::fs::read_to_string(entry.expect("entry").path()).expect("text"))
        .collect();
    texts.sort();
    texts
}

/// The app saves the note it last saw holding [`READ_BY_BOTH`] while a program
/// writes the same note, which it read too, between the save's last read and
/// its rename. Answers the save's result and the program's.
fn app_save_racing_a_program(
    fixture: &Fixture,
    lock: Option<&WriteLock>,
) -> (
    Result<(), StorageError>,
    Result<(), writ_core::notes::host::HostError>,
) {
    let last_seen = read_disk_state(&fixture.note)
        .expect("read")
        .expect("the note is there");
    let program = NoteHostImpl::open(
        &fixture.notes,
        None,
        [Capability::ReadNote, Capability::WriteNote]
            .into_iter()
            .collect(),
        FileExtension::Md,
    )
    .expect("open the host")
    .with_write_lock(lock.cloned());
    program
        .read_note("Launch.md")
        .expect("the program reads it");

    let (go, gone) = mpsc::channel::<()>();
    let (landed, has_landed) = mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        gone.recv().expect("the save lets the program go");
        let written = program
            .write_note(
                "Launch.md",
                PROGRAM_TEXT,
                LastKnown::LastSeen,
                WriteOrigin::Mcp {
                    client: "a program".to_string(),
                },
            )
            .map(|_| ());
        let _ = landed.send(());
        written
    });

    let unlocked = lock.is_none();
    let between_read_and_rename = |_: &Path, _: &[u8]| {
        go.send(()).expect("the program is waiting");
        // Unlocked, nothing holds the program off, so its write is waited
        // for however long it takes. Locked, it cannot land until the rename,
        // so the save gives it a while and goes on.
        if unlocked {
            let _ = has_landed.recv();
        } else {
            let _ = has_landed.recv_timeout(STAGE);
        }
    };
    let saved = write_note_guarded(
        GuardedWrite {
            target: &fixture.note,
            bytes: APP_TEXT.as_bytes(),
            last_known: Some(last_seen),
            on_disk: DiskRead::Fresh,
            dataless: None,
            origin: WriteOrigin::Editor,
            on_conflict: ConflictPolicy::RefuseWithCopy,
            history: None,
            lock,
        },
        Some(&between_read_and_rename),
    )
    .map(|_| ());
    (saved, writer.join().expect("the program's thread"))
}

// The race the lock closes, staged without it: the program's write lands
// between the save's last read and its rename, the rename replaces it, and
// nothing anywhere holds the program's text. This is the failure the test
// below holds the lock against.
#[test]
fn without_the_lock_a_write_landing_before_the_saves_rename_is_lost() {
    let fixture = fixture();

    let (saved, written) = app_save_racing_a_program(&fixture, None);

    saved.expect("the save lands");
    written.expect("the program was told its write landed");
    assert_eq!(texts_in(&fixture.notes), vec![APP_TEXT.to_string()]);
}

#[test]
fn an_app_save_racing_a_programs_write_keeps_both_texts() {
    let fixture = fixture();
    let lock = WriteLock::in_data_folder(&fixture.data);

    let (saved, written) = app_save_racing_a_program(&fixture, Some(&lock));

    saved.expect("the save lands");
    assert!(
        matches!(
            written,
            Err(writ_core::notes::host::HostError::Conflict {
                conflict_copy: Some(_),
                ..
            })
        ),
        "the program is told its text went beside the note: {written:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&fixture.note).expect("read"),
        APP_TEXT
    );
    assert_eq!(
        texts_in(&fixture.notes),
        vec![APP_TEXT.to_string(), PROGRAM_TEXT.to_string()],
        "both texts are in the folder"
    );
}

#[test]
fn a_save_that_waited_on_another_is_judged_on_what_the_file_holds_now() {
    let fixture = fixture();
    let lock = WriteLock::in_data_folder(&fixture.data);
    let last_seen = read_disk_state(&fixture.note)
        .expect("read")
        .expect("the note is there");

    let (go, gone) = mpsc::channel::<()>();
    let second = {
        let note = fixture.note.clone();
        let lock = lock.clone();
        std::thread::spawn(move || {
            gone.recv().expect("the first save lets this one go");
            write_note_guarded(
                GuardedWrite {
                    target: &note,
                    bytes: PROGRAM_TEXT.as_bytes(),
                    last_known: Some(last_seen),
                    on_disk: DiskRead::Fresh,
                    dataless: None,
                    origin: WriteOrigin::Editor,
                    on_conflict: ConflictPolicy::RefuseWithCopy,
                    history: None,
                    lock: Some(&lock),
                },
                None,
            )
            .map(|_| ())
        })
    };
    let hold_the_rename = |_: &Path, _: &[u8]| {
        go.send(()).expect("the second save is waiting");
        std::thread::sleep(STAGE);
    };
    write_note_guarded(
        GuardedWrite {
            target: &fixture.note,
            bytes: APP_TEXT.as_bytes(),
            last_known: Some(last_seen),
            on_disk: DiskRead::Fresh,
            dataless: None,
            origin: WriteOrigin::Editor,
            on_conflict: ConflictPolicy::RefuseWithCopy,
            history: None,
            lock: Some(&lock),
        },
        Some(&hold_the_rename),
    )
    .expect("the first save lands");

    let refused = second.join().expect("the second save's thread");
    assert!(
        matches!(refused, Err(StorageError::SourceChangedOnDisk { .. })),
        "{refused:?}"
    );
    assert_eq!(
        texts_in(&fixture.notes),
        vec![APP_TEXT.to_string(), PROGRAM_TEXT.to_string()]
    );
}

#[test]
fn a_held_lock_holds_off_a_second_holder_until_it_is_let_go() {
    let dir = TempDir::new().expect("temp dir");
    let lock = WriteLock::in_data_folder(dir.path());
    assert_eq!(lock.path(), dir.path().join(WRITE_LOCK_FILE));

    let held = lock.hold().expect("the lock is free");
    assert!(
        lock.hold_within(Duration::from_millis(50)).is_none(),
        "a second open of the lock waits and gives up"
    );
    drop(held);
    assert!(lock.hold_within(Duration::from_millis(50)).is_some());
    assert_eq!(
        std::fs::read(lock.path()).expect("the lock file").len(),
        0,
        "the lock file carries no data"
    );
}

#[test]
fn a_lock_that_cannot_be_had_does_not_stop_a_save() {
    let fixture = fixture();
    let unopenable = WriteLock::in_data_folder(&fixture.data.join("no such folder"));
    assert!(unopenable.hold().is_none());

    write_note_guarded(
        GuardedWrite {
            target: &fixture.note,
            bytes: APP_TEXT.as_bytes(),
            last_known: None,
            on_disk: DiskRead::Fresh,
            dataless: None,
            origin: WriteOrigin::Editor,
            on_conflict: ConflictPolicy::RefuseWithCopy,
            history: None,
            lock: Some(&unopenable),
        },
        None,
    )
    .expect("the save lands without the lock");
    assert_eq!(
        std::fs::read_to_string(&fixture.note).expect("read"),
        APP_TEXT
    );
}
