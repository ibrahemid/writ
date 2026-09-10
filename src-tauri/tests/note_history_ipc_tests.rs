//! Coverage for the four version commands (spec H1).
//!
//! Each is exercised through its Tauri-free inner function against a real
//! notes folder and a real version store, so the assertions cover what the
//! panel receives and what the folder holds afterwards. The last test asserts
//! every one of them is in the invoke handler, since a command that is not
//! registered cannot be called however well it behaves.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;
use writ_core::activity::{Actor, Decision};
use writ_core::notes::guard::DiskState;
use writ_storage::note_history::NoteHistoryStore;
use writ_tauri_lib::commands::note_history::{
    copy_note_version_inner, note_version_content_inner, note_versions_inner,
    restore_note_version_inner, NoteVersion,
};
use writ_tauri_lib::watcher::identity::PlatformIdentity;

const LIB_RS: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "commands::note_history::note_versions",
    "commands::note_history::note_version_content",
    "commands::note_history::restore_note_version",
    "commands::note_history::copy_note_version",
];

/// A notes folder and the store that keeps its versions, wired the way the app
/// wires them.
struct Folder {
    dir: TempDir,
    notes: PathBuf,
    writ_dir: PathBuf,
    store: NoteHistoryStore,
}

impl Folder {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let writ_dir = dir.path().join("data");
        std::fs::create_dir_all(&writ_dir).expect("data folder");
        let notes = writ_dir.join("Writ");
        std::fs::create_dir_all(&notes).expect("notes folder");
        let notes = writ_tauri_lib::security::canonicalize_root(&notes).expect("canonical");

        let store = NoteHistoryStore::open(&writ_dir).expect("version store");
        store.set_notes_root(notes.clone());
        store.set_probe(Arc::new(PlatformIdentity));
        Self {
            dir,
            notes,
            writ_dir,
            store,
        }
    }

    /// Writes a note and keeps that text, at `seconds_ago`.
    fn note(&self, name: &str, bytes: &[u8], seconds_ago: u64) -> PathBuf {
        let path = self.notes.join(name);
        std::fs::write(&path, bytes).expect("write");
        self.keep(&path, bytes, seconds_ago);
        path
    }

    /// Keeps one text of a note, without the merge window: a fixture wants
    /// each text it hands over to be an entry of its own.
    fn keep(&self, path: &Path, bytes: &[u8], seconds_ago: u64) {
        let key = self.store.key_for(path).expect("a note inside the folder");
        let at = SystemTime::now() - Duration::from_secs(seconds_ago);
        self.store
            .capture_replaced(&key, bytes, at)
            .expect("keep the text");
    }

    fn versions(&self, path: &Path) -> Vec<NoteVersion> {
        note_versions_inner(&self.store, path).expect("versions")
    }

    fn text_of(&self, version: &NoteVersion) -> String {
        note_version_content_inner(&self.store, version.id).expect("the text of an entry")
    }

    /// What Writ would have recorded for a tab holding these bytes.
    fn last_known(bytes: &[u8]) -> DiskState {
        DiskState {
            hash: writ_core::hash::sha256_bytes(bytes),
            size: bytes.len() as u64,
            mtime: None,
        }
    }

    /// The names the folder holds, sorted.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.notes)
            .expect("read the folder")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

#[test]
fn note_versions_lists_what_a_note_held_newest_first() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    folder.keep(&path, b"the second\n", 30);

    let listed = folder.versions(&path);

    assert_eq!(listed.len(), 2);
    assert_eq!(folder.text_of(&listed[0]), "the second\n");
    assert_eq!(folder.text_of(&listed[1]), "the first\n");
    assert!(listed[0].at_ms > listed[1].at_ms);
    assert_eq!(listed[0].bytes, b"the second\n".len() as u64);
}

#[test]
fn a_file_the_notes_folder_does_not_hold_has_no_versions() {
    let folder = Folder::new();
    let outside = folder.dir.path().join("elsewhere.md");
    std::fs::write(&outside, "not a note\n").expect("write");

    assert!(note_versions_inner(&folder.store, &outside)
        .expect("an answer, not a failure")
        .is_empty());
}

#[test]
fn note_version_content_reads_the_text_an_entry_holds() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"what it said\n", 60);

    let listed = folder.versions(&path);

    assert_eq!(folder.text_of(&listed[0]), "what it said\n");
}

#[test]
fn an_entry_that_is_gone_reads_as_gone_rather_than_as_a_failure_of_the_store() {
    let folder = Folder::new();

    let error = note_version_content_inner(&folder.store, 4_242).expect_err("no such entry");

    assert_eq!(error, "That version is not here any more.");
}

#[test]
fn restoring_a_version_writes_it_back_and_keeps_the_text_it_replaced() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    std::fs::write(&path, b"the second\n").expect("overwrite");
    folder.keep(&path, b"the second\n", 30);

    let listed = folder.versions(&path);
    let first = listed[1].id;

    let restored =
        restore_note_version_inner(&folder.notes, &folder.writ_dir, &folder.store, first, None)
            .expect("restore");

    assert_eq!(restored.note, "Launch.md");
    assert_eq!(std::fs::read(&path).expect("read"), b"the first\n");

    let after = folder.versions(&path);
    assert_eq!(
        folder.text_of(&after[0]),
        "the first\n",
        "the restore is an entry of its own"
    );
    assert!(
        after
            .iter()
            .any(|entry| folder.text_of(entry) == "the second\n"),
        "and what it replaced is still there to go back to"
    );
}

#[test]
fn restoring_twice_returns_the_note_to_where_it_started() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    std::fs::write(&path, b"the second\n").expect("overwrite");
    folder.keep(&path, b"the second\n", 30);

    let first = folder.versions(&path)[1].id;
    restore_note_version_inner(&folder.notes, &folder.writ_dir, &folder.store, first, None)
        .expect("restore");

    let back = folder
        .versions(&path)
        .into_iter()
        .find(|entry| folder.text_of(entry) == "the second\n")
        .expect("the text the restore replaced");
    restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        back.id,
        None,
    )
    .expect("restore back");

    assert_eq!(std::fs::read(&path).expect("read"), b"the second\n");
}

#[test]
fn a_note_changed_underneath_is_refused_and_the_version_is_written_beside_it() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    let last_known = Folder::last_known(b"the first\n");
    // Somebody else writes the file, and Writ never read what they wrote.
    std::fs::write(&path, b"what somebody else wrote\n").expect("overwrite");

    let version = folder.versions(&path)[0].id;
    let error = restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        Some(last_known),
    )
    .expect_err("refused");

    assert!(
        error.starts_with("Launch.md changed on disk."),
        "got: {error}"
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"what somebody else wrote\n",
        "the file keeps what it holds"
    );
    let copy = folder
        .names()
        .into_iter()
        .find(|name| name != "Launch.md")
        .expect("a copy beside the note");
    assert!(error.contains(&copy), "the refusal names it: {error}");
    assert_eq!(
        std::fs::read(folder.notes.join(&copy)).expect("read the copy"),
        b"the first\n"
    );
}

#[test]
fn a_restore_puts_back_every_byte_it_was_given() {
    let folder = Folder::new();
    // Frontmatter, CRLF line endings and a byte order mark: everything a
    // round trip through a string would quietly rewrite (spec 163).
    let original: &[u8] =
        b"\xef\xbb\xbf---\r\ntitle: Launch\r\ntags: [a, b]\r\n---\r\n\r\nBody\r\n";
    let path = folder.note("Launch.md", original, 60);
    std::fs::write(&path, "something else\n").expect("overwrite");

    let version = folder.versions(&path)[0].id;
    restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        None,
    )
    .expect("restore");

    assert_eq!(std::fs::read(&path).expect("read"), original);
}

#[test]
fn copying_a_version_leaves_the_note_alone() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    std::fs::write(&path, b"the second\n").expect("overwrite");
    folder.keep(&path, b"the second\n", 30);

    let first = folder.versions(&path)[1].id;
    let copy = copy_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        first,
        chrono::Utc::now(),
    )
    .expect("copy");

    assert!(copy.name.starts_with("Launch ("), "got: {}", copy.name);
    assert!(copy.name.ends_with(".md"), "got: {}", copy.name);
    assert_eq!(
        std::fs::read(folder.notes.join(&copy.name)).expect("read the copy"),
        b"the first\n"
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"the second\n",
        "the note is untouched"
    );
}

#[test]
fn a_restore_and_a_copy_each_leave_a_line_in_the_activity_log_naming_no_text() {
    let folder = Folder::new();
    let text = b"the only text this note ever held\n";
    let path = folder.note("Launch.md", text, 60);
    let version = folder.versions(&path)[0].id;

    restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        None,
    )
    .expect("restore");
    copy_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        chrono::Utc::now(),
    )
    .expect("copy");

    let lines = writ_storage::activity_log::read_recent(&folder.writ_dir, 10);
    for action in ["restore_note_version", "copy_note_version"] {
        let line = lines
            .iter()
            .find(|record| record.action == action)
            .unwrap_or_else(|| panic!("{action} left no line: {lines:?}"));
        assert_eq!(line.actor, Actor::App);
        assert_eq!(line.decision, Decision::Allow);
        assert_eq!(line.path.as_deref(), Some(Path::new("Launch.md")));
        assert_eq!(line.bytes, Some(text.len() as u64));
    }

    let written = format!("{lines:?}");
    assert!(
        !written.contains("the only text this note ever held"),
        "a log line holds a length and a name, never what the note said: {written}"
    );
}

#[test]
fn every_version_command_is_in_the_invoke_handler() {
    for command in COMMANDS {
        assert!(
            LIB_RS.contains(command),
            "{command} is not registered in the invoke handler"
        );
    }
}
