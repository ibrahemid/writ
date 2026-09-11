//! The version store, asserted against a real directory and a real database.
//!
//! What the policy decides is covered in `writ-core`; what is covered here is
//! where the texts land, that they never land in the notes folder, that a
//! note keeps its history when its file is renamed or its folder is moved,
//! and that pruning gives the disk back rather than only the index rows.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;
use writ_core::note_history::{VersionKey, MAX_VERSIONS_PER_NOTE};
use writ_core::notes::identity::{FileIdentity, IdentityProbe};
use writ_core::notes::WriteOrigin;
use writ_storage::errors::StorageError;
use writ_storage::note_history::{Kept, NoteHistoryStore};

/// The platform's answer on Unix, and a description everywhere else.
///
/// The app's own probe (`src-tauri/src/watcher/identity.rs`) is not reachable
/// from this crate, so the tests carry the two lines of it they need.
struct Probe;

impl IdentityProbe for Probe {
    #[cfg(unix)]
    fn identity_of(&self, path: &Path) -> Option<FileIdentity> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).ok()?;
        metadata.is_file().then(|| FileIdentity::Inode {
            dev: metadata.dev(),
            ino: metadata.ino(),
            birth_ns: None,
        })
    }

    #[cfg(not(unix))]
    fn identity_of(&self, path: &Path) -> Option<FileIdentity> {
        let metadata = std::fs::metadata(path).ok()?;
        metadata.is_file().then(|| FileIdentity::Fallback {
            path: path.to_string_lossy().into_owned(),
            size: metadata.len(),
            mtime_ms: None,
            hash: writ_core::hash::sha256_bytes(&std::fs::read(path).unwrap_or_default()),
        })
    }
}

/// A volume with no stable id to give, which is FAT, exFAT and some SMB
/// servers ([`FileIdentity::Fallback`]).
struct NoStableId;

impl IdentityProbe for NoStableId {
    fn identity_of(&self, path: &Path) -> Option<FileIdentity> {
        let bytes = std::fs::read(path).ok()?;
        Some(FileIdentity::Fallback {
            path: path.to_string_lossy().into_owned(),
            size: bytes.len() as u64,
            mtime_ms: Some(1),
            hash: writ_core::hash::sha256_bytes(&bytes),
        })
    }
}

/// A data directory, a notes folder beside it, and a store over both.
struct Fixture {
    _home: TempDir,
    writ_dir: PathBuf,
    notes: PathBuf,
    store: NoteHistoryStore,
}

impl Fixture {
    fn new() -> Self {
        Self::with_probe(Arc::new(Probe))
    }

    fn with_probe(probe: Arc<dyn IdentityProbe>) -> Self {
        let home = TempDir::new().expect("temp dir");
        let writ_dir = home.path().join("writ-data");
        let notes = home.path().join("Writ");
        std::fs::create_dir_all(&writ_dir).expect("data dir");
        std::fs::create_dir_all(&notes).expect("notes dir");
        let store = NoteHistoryStore::open(&writ_dir).expect("open");
        store.set_notes_root(notes.clone());
        store.set_probe(probe);
        Self {
            _home: home,
            writ_dir,
            notes,
            store,
        }
    }

    /// Writes a note and hands back its path.
    fn note(&self, name: &str, text: &str) -> PathBuf {
        let path = self.notes.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("note folder");
        }
        std::fs::write(&path, text).expect("write note");
        path
    }

    fn key(&self, path: &Path) -> VersionKey {
        self.store.key_for(path).expect("a note inside the folder")
    }
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// Every file under `root`, so a test can say what a folder does not hold.
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[test]
fn a_text_is_kept_and_comes_back_byte_for_byte() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "---\ntags: [a]\n---\n\nthe body\r\n");
    let key = fixture.key(&path);
    let bytes = std::fs::read(&path).expect("read");

    let id = fixture
        .store
        .capture(&key, &bytes, at(1_000), &WriteOrigin::Editor)
        .expect("capture")
        .entry()
        .expect("an entry");

    assert_eq!(fixture.store.content(id).expect("content"), bytes);
}

#[test]
fn the_texts_live_under_the_data_directory_and_never_in_the_notes_folder() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "one\n");
    let key = fixture.key(&path);
    fixture
        .store
        .capture(&key, b"one\n", at(1_000), &WriteOrigin::Editor)
        .expect("capture");
    fixture
        .store
        .capture(&key, b"two\n", at(2_000), &WriteOrigin::Editor)
        .expect("capture");

    let in_notes = walk(&fixture.notes);
    assert_eq!(
        in_notes,
        vec![path],
        "the notes folder holds the note and nothing else"
    );
    let kept = walk(&fixture.writ_dir.join("history"));
    assert_eq!(kept.len(), 2, "one file per text, sharded: {kept:?}");
    for text in &kept {
        let shard = text.parent().expect("a shard folder");
        assert_eq!(
            shard
                .file_name()
                .expect("shard name")
                .to_string_lossy()
                .len(),
            2,
            "the shard is the first byte of the digest"
        );
    }
    assert!(fixture.writ_dir.join("history.db").exists());
}

#[test]
fn a_run_of_saves_inside_the_window_is_one_entry_and_the_next_one_is_another() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "draft\n");
    let key = fixture.key(&path);

    let start = at(1_000);
    for save in 0..100u64 {
        let text = format!("draft {save}\n");
        fixture
            .store
            .capture(
                &key,
                text.as_bytes(),
                start + Duration::from_millis(save * 90),
                &WriteOrigin::Editor,
            )
            .expect("capture");
    }
    assert_eq!(fixture.store.versions(&key).expect("versions").len(), 1);

    fixture
        .store
        .capture(
            &key,
            b"the eleventh second\n",
            start + Duration::from_secs(11),
            &WriteOrigin::Editor,
        )
        .expect("capture");
    assert_eq!(fixture.store.versions(&key).expect("versions").len(), 2);
}

#[test]
fn a_write_that_is_not_the_editors_is_never_absorbed_by_a_run_of_saves() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "the first\n");
    let key = fixture.key(&path);
    let start = at(1_000);

    fixture
        .store
        .capture(&key, b"the second\n", start, &WriteOrigin::Editor)
        .expect("save");
    // Three seconds later, well inside the window a save would have joined.
    let put_back = fixture
        .store
        .capture(
            &key,
            b"the first\n",
            start + Duration::from_secs(3),
            &WriteOrigin::Restore,
        )
        .expect("restore");

    assert!(
        matches!(put_back, Kept::Added(_)),
        "a restore is a version of its own: {put_back:?}"
    );
    let texts: Vec<Vec<u8>> = fixture
        .store
        .versions(&key)
        .expect("versions")
        .into_iter()
        .map(|entry| fixture.store.content(entry.id).expect("text"))
        .collect();
    assert_eq!(
        texts,
        vec![b"the first\n".to_vec(), b"the second\n".to_vec()],
        "the text the restore landed on is still there to go back to"
    );
}

#[test]
fn a_run_of_autosaves_inside_the_window_is_one_entry_like_a_run_of_saves() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "draft\n");
    let key = fixture.key(&path);

    let start = at(1_000);
    for tick in 0..20u64 {
        let text = format!("draft {tick}\n");
        fixture
            .store
            .capture(
                &key,
                text.as_bytes(),
                start + Duration::from_millis(tick * 400),
                &WriteOrigin::Autosave,
            )
            .expect("capture");
    }

    assert_eq!(
        fixture.store.versions(&key).expect("versions").len(),
        1,
        "an autosave is the editor writing, so a run of them collapses the way a run of saves does"
    );
}

#[test]
fn an_idle_save_storm_costs_nothing() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "unchanged\n");
    let key = fixture.key(&path);

    for save in 0..50u64 {
        fixture
            .store
            .capture(
                &key,
                b"unchanged\n",
                at(1_000 + save * 60),
                &WriteOrigin::Editor,
            )
            .expect("capture");
    }
    assert_eq!(
        fixture.store.versions(&key).expect("versions").len(),
        1,
        "the same text is kept once however often it is saved"
    );
    assert_eq!(walk(&fixture.writ_dir.join("history")).len(), 1);
}

#[test]
fn a_text_about_to_be_replaced_is_kept_even_inside_the_window() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "mine\n");
    let key = fixture.key(&path);

    fixture
        .store
        .capture(&key, b"mine\n", at(1_000), &WriteOrigin::Editor)
        .expect("capture");
    fixture
        .store
        .capture_replaced(&key, b"somebody else's\n", at(1_002))
        .expect("capture");

    let versions = fixture.store.versions(&key).expect("versions");
    assert_eq!(
        versions.len(),
        2,
        "the merge window never drops a text with nowhere else to be"
    );
}

#[test]
fn a_note_over_the_ceiling_is_not_versioned_at_all() {
    let fixture = Fixture::new();
    let path = fixture.note("Big.md", "x");
    let key = fixture.key(&path);
    let big = vec![b'x'; 3 * 1024 * 1024];

    assert_eq!(
        fixture
            .store
            .capture(&key, &big, at(1_000), &WriteOrigin::Editor)
            .expect("capture"),
        Kept::Nothing
    );
    assert!(fixture.store.versions(&key).expect("versions").is_empty());
    assert!(walk(&fixture.writ_dir.join("history")).is_empty());
}

#[test]
fn a_file_the_notes_folder_does_not_hold_has_no_key() {
    let fixture = Fixture::new();
    let stranger = fixture._home.path().join("elsewhere.md");
    std::fs::write(&stranger, "not a note\n").expect("write");

    assert!(fixture.store.key_for(&stranger).is_none());
}

#[test]
fn a_note_renamed_inside_the_folder_keeps_its_history_through_its_identity() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "one\n");
    let key = fixture.key(&path);
    fixture
        .store
        .capture(&key, b"one\n", at(1_000), &WriteOrigin::Editor)
        .expect("capture");

    let renamed = fixture.notes.join("Ship it.md");
    std::fs::rename(&path, &renamed).expect("rename");

    let after = fixture.store.key_for(&renamed).expect("key");
    assert_ne!(after.path, key.path, "the note is at another name now");
    let versions = fixture.store.versions(&after).expect("versions");
    assert_eq!(versions.len(), 1, "the history followed the file");
    assert_eq!(
        fixture.store.content(versions[0].id).expect("content"),
        b"one\n"
    );
}

#[test]
fn moving_the_notes_folder_keeps_every_notes_history() {
    let fixture = Fixture::new();
    let path = fixture.note("Ideas/Launch.md", "one\n");
    let key = fixture.key(&path);
    fixture
        .store
        .capture(&key, b"one\n", at(1_000), &WriteOrigin::Editor)
        .expect("capture");

    let moved = fixture._home.path().join("Notes moved");
    std::fs::rename(&fixture.notes, &moved).expect("move the folder");
    fixture.store.set_notes_root(moved.clone());

    let after = fixture
        .store
        .key_for(&moved.join("Ideas/Launch.md"))
        .expect("key");
    assert_eq!(
        fixture.store.versions(&after).expect("versions").len(),
        1,
        "history is keyed by identity and a folder-relative name, not by where the folder sits"
    );
}

#[test]
fn a_note_that_took_a_name_another_one_left_lists_what_that_name_held() {
    // The name is what a person asks about, so the name is what answers. A
    // note is the file at a path, and every atomic save by any editor puts a
    // new file there: an id that stopped matching cannot be read as a
    // different note without splitting one note's history on every save
    // somebody else's editor makes. The texts of the note that moved away are
    // still there to restore, and they rejoin it under its own name the next
    // time it is written.
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "the first\n");
    let key = fixture.key(&path);
    fixture
        .store
        .capture(&key, b"the first\n", at(1_000), &WriteOrigin::Editor)
        .expect("capture");
    std::fs::rename(&path, fixture.notes.join("Launch old.md")).expect("rename");

    let second = fixture.note("Launch.md", "a different note\n");
    let second_key = fixture.store.key_for(&second).expect("key");
    fixture
        .store
        .capture(
            &second_key,
            b"a different note\n",
            at(2_000),
            &WriteOrigin::Editor,
        )
        .expect("capture");

    let versions = fixture.store.versions(&second_key).expect("versions");
    assert_eq!(
        versions.len(),
        2,
        "the name keeps everything written under it"
    );
    assert_eq!(
        fixture.store.content(versions[0].id).expect("content"),
        b"a different note\n",
        "and the newest is the one just written"
    );
}

#[test]
fn a_volume_with_no_stable_id_still_keeps_a_notes_history_under_its_path() {
    let fixture = Fixture::with_probe(Arc::new(NoStableId));
    let path = fixture.note("Launch.md", "one\n");
    let key = fixture.key(&path);
    assert!(
        key.durable_identity().is_none(),
        "the fallback is what a volume with no id is described by"
    );

    fixture
        .store
        .capture(&key, b"one\n", at(1_000), &WriteOrigin::Editor)
        .expect("capture");
    std::fs::write(&path, "two\n").expect("write");
    let later = fixture.store.key_for(&path).expect("key");
    fixture
        .store
        .capture(&later, b"two\n", at(2_000), &WriteOrigin::Editor)
        .expect("capture");

    assert_eq!(
        fixture.store.versions(&later).expect("versions").len(),
        2,
        "the path is the whole of the key where nothing else can be read"
    );
}

#[test]
fn a_note_whose_file_is_gone_still_answers_for_its_history() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "the last thing it said\n");
    let key = fixture.key(&path);
    fixture
        .store
        .capture(
            &key,
            b"the last thing it said\n",
            at(1_000),
            &WriteOrigin::Editor,
        )
        .expect("capture");
    std::fs::remove_file(&path).expect("delete");

    let after = fixture.store.key_for(&path).expect("key");
    assert!(
        after.identity.is_none(),
        "a file that is gone describes nothing"
    );
    let versions = fixture.store.versions(&after).expect("versions");
    assert_eq!(versions.len(), 1);
    assert_eq!(
        fixture.store.content(versions[0].id).expect("content"),
        b"the last thing it said\n"
    );
}

#[test]
fn an_entry_that_is_not_there_is_a_typed_error() {
    let fixture = Fixture::new();
    match fixture.store.content(404) {
        Err(StorageError::VersionMissing { id }) => assert_eq!(id, 404),
        other => panic!("expected a missing version, got {other:?}"),
    }
}

#[test]
fn pruning_gives_back_the_disk_and_not_only_the_rows() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "one\n");
    let key = fixture.key(&path);
    let old = at(1_000);
    for save in 0..5u64 {
        let text = format!("draft {save}\n");
        fixture
            .store
            .capture(
                &key,
                text.as_bytes(),
                old + Duration::from_secs(save * 60),
                &WriteOrigin::Editor,
            )
            .expect("capture");
    }
    assert_eq!(walk(&fixture.writ_dir.join("history")).len(), 5);

    let outcome = fixture
        .store
        .prune(old + Duration::from_secs(40 * 24 * 60 * 60))
        .expect("prune");

    assert_eq!(outcome.retired, 4, "the newest is the one a note keeps");
    assert_eq!(outcome.texts_deleted, 4);
    assert!(outcome.bytes_freed > 0);
    assert_eq!(
        walk(&fixture.writ_dir.join("history")).len(),
        1,
        "a retired entry's text is deleted, not left behind"
    );
    assert_eq!(fixture.store.versions(&key).expect("versions").len(), 1);
}

#[test]
fn two_entries_holding_one_text_keep_it_until_both_are_gone() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "one\n");
    let key = fixture.key(&path);
    // There and back again: two entries, one text.
    fixture
        .store
        .capture(&key, b"one\n", at(1_000), &WriteOrigin::Editor)
        .expect("capture");
    fixture
        .store
        .capture(&key, b"two\n", at(2_000), &WriteOrigin::Editor)
        .expect("capture");
    fixture
        .store
        .capture(&key, b"one\n", at(3_000), &WriteOrigin::Editor)
        .expect("capture");
    assert_eq!(fixture.store.versions(&key).expect("versions").len(), 3);
    assert_eq!(walk(&fixture.writ_dir.join("history")).len(), 2);

    fixture
        .store
        .prune(at(3_000) + Duration::from_secs(40 * 24 * 60 * 60))
        .expect("prune");

    let kept = fixture.store.versions(&key).expect("versions");
    assert_eq!(kept.len(), 1);
    assert_eq!(
        fixture.store.content(kept[0].id).expect("content"),
        b"one\n"
    );
}

#[test]
fn ten_thousand_saves_of_one_note_leave_two_hundred_versions_and_a_small_store() {
    let fixture = Fixture::new();
    let path = fixture.note("Launch.md", "start\n");
    let key = fixture.key(&path);

    let start = at(1_000);
    // 20 KB, and a different 20 KB every time: nothing here is deduped away.
    for save in 0..10_000u64 {
        let mut text = format!("save {save}\n").into_bytes();
        text.resize(20 * 1024, b'x');
        text.extend_from_slice(save.to_string().as_bytes());
        fixture
            .store
            .capture(
                &key,
                &text,
                start + Duration::from_secs(save * 60),
                &WriteOrigin::Editor,
            )
            .expect("capture");
    }
    let last = start + Duration::from_secs(10_000 * 60);
    fixture.store.prune(last).expect("prune");

    let versions = fixture.store.versions(&key).expect("versions");
    assert!(
        versions.len() <= MAX_VERSIONS_PER_NOTE,
        "{} versions is over the cap",
        versions.len()
    );
    let on_disk: u64 = walk(&fixture.writ_dir)
        .iter()
        .filter_map(|path| path.metadata().ok())
        .map(|meta| meta.len())
        .sum();
    assert!(
        on_disk < 25 * 1024 * 1024,
        "the whole store is {on_disk} bytes"
    );
    assert_eq!(
        walk(&fixture.notes),
        vec![path],
        "ten thousand saves put nothing in the notes folder"
    );
}

#[cfg(windows)]
#[test]
fn a_notes_folder_spelled_as_the_app_carries_it_still_names_its_notes() {
    let fixture = Fixture::new();
    let path = fixture.note("Ideas/Launch.md", "one\n");
    // What `std::fs::canonicalize` hands back on Windows, which is not how
    // `AppState` spells the same folder (U7's CI round).
    let verbatim = std::fs::canonicalize(&fixture.notes).expect("canonicalize");
    fixture.store.set_notes_root(verbatim);

    let key = fixture
        .store
        .key_for(&std::fs::canonicalize(&path).expect("canonicalize"))
        .expect("a note inside the folder");
    assert_eq!(
        key.path,
        PathBuf::from("Ideas/Launch.md"),
        "one spelling of a note's name on every platform"
    );
}
