//! Creating, renaming, trashing and copying the files notes live in
//! (ADR-028 §3).

use std::path::Path;

use tempfile::TempDir;
use writ_core::hash::sha256_bytes;
use writ_core::notes::guard::DiskState;
use writ_storage::errors::StorageError;
use writ_storage::note_ops;

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

#[test]
fn create_note_writes_a_file_immediately() {
    let root = TempDir::new().expect("temp dir");

    let path = note_ops::create_note(root.path(), "2026-08-29", None).expect("create");

    assert!(path.exists(), "{} was not created", path.display());
    assert_eq!(path, root.path().join("2026-08-29.md"));
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "");
}

#[test]
fn create_note_dedupes_against_an_existing_name() {
    let root = TempDir::new().expect("temp dir");
    std::fs::write(root.path().join("Notes.md"), "first").expect("seed");

    let path = note_ops::create_note(root.path(), "Notes", None).expect("create");

    assert_eq!(path, root.path().join("Notes 2.md"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("Notes.md")).expect("read"),
        "first",
        "the note that was already there was written over"
    );
}

#[test]
fn rename_note_moves_the_file_and_the_old_name_is_gone() {
    let root = TempDir::new().expect("temp dir");
    let from = root.path().join("2026-08-29.md");
    std::fs::write(&from, "the text").expect("seed");
    let last_known = recorded(&from);

    let to = note_ops::rename_note(&from, "Grocery list", Some(last_known), None).expect("rename");

    assert_eq!(to, root.path().join("Grocery list.md"));
    assert!(!from.exists(), "the old name is still there");
    assert_eq!(std::fs::read_to_string(&to).expect("read"), "the text");
}

#[test]
fn rename_to_a_colliding_name_refuses_and_names_the_collision() {
    let root = TempDir::new().expect("temp dir");
    let from = root.path().join("2026-08-29.md");
    std::fs::write(&from, "the text").expect("seed");
    let taken = root.path().join("Grocery list.md");
    std::fs::write(&taken, "somebody else's").expect("seed");
    let last_known = recorded(&from);

    let error = note_ops::rename_note(&from, "Grocery list", Some(last_known), None)
        .expect_err("the rename should stop");

    match error {
        StorageError::NoteNameTaken { name, folder } => {
            assert_eq!(name, "Grocery list.md");
            assert_eq!(folder, root.path());
        }
        other => panic!("expected a taken name, got {other:?}"),
    }
    assert!(from.exists(), "the note being renamed was moved anyway");
    assert_eq!(
        std::fs::read_to_string(&taken).expect("read"),
        "somebody else's",
        "the file already there was written over"
    );
}

#[test]
fn rename_to_an_empty_name_refuses() {
    let root = TempDir::new().expect("temp dir");
    let from = root.path().join("2026-08-29.md");
    std::fs::write(&from, "the text").expect("seed");

    let error =
        note_ops::rename_note(&from, "   ", Some(recorded(&from)), None).expect_err("no name");

    assert!(matches!(error, StorageError::NoteNameEmpty), "{error:?}");
    assert!(from.exists());
}

#[test]
fn rename_goes_through_the_disk_state_guard() {
    let root = TempDir::new().expect("temp dir");
    let from = root.path().join("2026-08-29.md");
    std::fs::write(&from, "what Writ read").expect("seed");
    let last_known = recorded(&from);
    std::fs::write(&from, "what somebody else wrote").expect("write");

    let error = note_ops::rename_note(&from, "Grocery list", Some(last_known), None)
        .expect_err("the rename should stop");

    match error {
        StorageError::SourceChangedOnDisk {
            path,
            conflict_copy,
            ..
        } => {
            assert_eq!(path, from.to_string_lossy());
            assert!(
                conflict_copy.is_none(),
                "a rename carries no text, so it writes no copy"
            );
        }
        other => panic!("expected a changed file, got {other:?}"),
    }
    assert!(
        from.exists(),
        "the file was moved out from under the change"
    );
    assert_eq!(
        std::fs::read_to_string(&from).expect("read"),
        "what somebody else wrote"
    );
    assert!(!root.path().join("Grocery list.md").exists());
}

#[test]
fn trash_note_does_not_unlink() {
    let root = TempDir::new().expect("temp dir");
    let path = root.path().join("2026-08-29.md");
    std::fs::write(&path, "the text").expect("seed");

    note_ops::trash_note(&path).expect("trash");

    assert!(!path.exists(), "the note is still at its path");

    // A real Trash cannot be asserted from a test, so the guarantee that the
    // note went there rather than being unlinked is enforced on the source:
    // nothing in the module may reach `remove_file`.
    let source =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/note_ops.rs"))
            .expect("read the module");
    let code = source
        .split_once("\n#[cfg(test)]")
        .map(|(before, _)| before)
        .unwrap_or(&source);
    assert!(
        !code.contains("remove_file"),
        "note_ops reaches remove_file; a deleted note has to be recoverable"
    );
    assert!(!code.contains("remove_dir"), "note_ops reaches remove_dir");
}

#[test]
fn save_copy_writes_into_the_notes_folder_and_leaves_the_original_untouched() {
    let root = TempDir::new().expect("temp dir");
    let elsewhere = TempDir::new().expect("temp dir");
    let original = elsewhere.path().join("report.md");
    std::fs::write(&original, "the text").expect("seed");

    let copy = note_ops::save_copy(root.path(), "report", "the text", None).expect("copy");

    assert_eq!(copy, root.path().join("report.md"));
    assert_eq!(std::fs::read_to_string(&copy).expect("read"), "the text");
    assert!(original.exists(), "the file the copy came from is gone");
    assert_eq!(
        std::fs::read_to_string(&original).expect("read"),
        "the text"
    );
}
