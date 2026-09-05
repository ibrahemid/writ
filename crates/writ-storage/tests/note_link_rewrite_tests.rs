//! Rewriting the links that name a renamed note (spec L3).
//!
//! The pure rewrite is `writ_core::notes::rename`; what is asserted here is
//! the half that touches the disk: which files are refused before anything is
//! written, and that every write that does happen is stamped first.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_core::hash::sha256_bytes;
use writ_core::notes::guard::{DiskState, SF_DATALESS};
use writ_core::notes::links::WikilinkTarget;
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

fn target(name: &str) -> WikilinkTarget {
    WikilinkTarget {
        name: name.to_string(),
        ..WikilinkTarget::default()
    }
}

/// A note holding one link to `Old note`, written into a fresh folder.
fn seeded(text: &str) -> (TempDir, PathBuf) {
    let root = TempDir::new().expect("temp dir");
    let path = root.path().join("Linking note.md");
    std::fs::write(&path, text).expect("seed");
    (root, path)
}

#[test]
fn a_link_is_rewritten_in_place() {
    let (_root, path) = seeded("see [[Old note|the one]] for more\n");

    let written =
        note_ops::rewrite_links_in_file(&path, &target("Old note"), "New note", None, None, None)
            .expect("the rewrite should land");

    assert!(written);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "see [[New note|the one]] for more\n"
    );
}

#[test]
fn a_file_naming_the_note_nowhere_is_not_written() {
    let (_root, path) = seeded("nothing links anywhere here\n");
    let before = recorded(&path);

    let written =
        note_ops::rewrite_links_in_file(&path, &target("Old note"), "New note", None, None, None)
            .expect("nothing to do is not a failure");

    assert!(!written);
    assert_eq!(recorded(&path).hash, before.hash);
}

#[test]
fn a_file_that_is_not_downloaded_is_refused_before_it_is_read() {
    // The probe stands in for `SF_DATALESS`, so the refusal is asserted on
    // every platform rather than only where the flag can be set.
    let (_root, path) = seeded("see [[Old note]]\n");
    let probe = |_: &Path| Some(SF_DATALESS);

    let error = note_ops::rewrite_links_in_file(
        &path,
        &target("Old note"),
        "New note",
        None,
        Some(&probe),
        None,
    )
    .expect_err("an evicted file should be refused");

    match error {
        StorageError::SourceNotDownloaded { path: named } => {
            assert_eq!(named, path.to_string_lossy());
        }
        other => panic!("expected a file that is not downloaded, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "see [[Old note]]\n",
        "the file was rewritten anyway"
    );
}

#[test]
fn a_file_changed_underneath_is_refused() {
    let (_root, path) = seeded("see [[Old note]]\n");
    let last_known = recorded(&path);
    std::fs::write(&path, "somebody else wrote this, and [[Old note]]\n").expect("write");

    let error = note_ops::rewrite_links_in_file(
        &path,
        &target("Old note"),
        "New note",
        Some(last_known),
        None,
        None,
    )
    .expect_err("a file changed underneath should be refused");

    match error {
        StorageError::SourceChangedOnDisk {
            path: named,
            conflict_copy,
            ..
        } => {
            assert_eq!(named, path.to_string_lossy());
            assert!(
                conflict_copy.is_none(),
                "a rewrite that never happened sets nothing aside"
            );
        }
        other => panic!("expected a changed file, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "somebody else wrote this, and [[Old note]]\n"
    );
}

#[test]
fn a_file_unchanged_since_writ_read_it_is_rewritten() {
    let (_root, path) = seeded("see [[Old note]]\n");
    let last_known = recorded(&path);

    let written = note_ops::rewrite_links_in_file(
        &path,
        &target("Old note"),
        "New note",
        Some(last_known),
        None,
        None,
    )
    .expect("the guard should let this through");

    assert!(written);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "see [[New note]]\n"
    );
}

#[cfg(unix)]
#[test]
fn a_read_only_file_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, path) = seeded("see [[Old note]]\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");

    let error =
        note_ops::rewrite_links_in_file(&path, &target("Old note"), "New note", None, None, None)
            .expect_err("a read-only file should be refused");

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    match error {
        StorageError::DestinationReadOnly { path: named } => {
            assert_eq!(named, path.display().to_string());
        }
        other => panic!("expected a read-only destination, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "see [[Old note]]\n"
    );
}

#[test]
fn the_rewrite_is_stamped_before_it_lands() {
    // The watcher reads an unstamped write of Writ's own as somebody else's
    // edit, which puts an external-change prompt in front of a person who
    // pressed rename.
    let (_root, path) = seeded("see [[Old note]]\n");
    let stamped: std::cell::RefCell<Vec<(PathBuf, String)>> = std::cell::RefCell::new(Vec::new());
    let stamp = |target: &Path, bytes: &[u8]| {
        stamped.borrow_mut().push((
            target.to_path_buf(),
            std::fs::read_to_string(target).unwrap_or_default(),
        ));
        assert_eq!(
            String::from_utf8_lossy(bytes),
            "see [[New note]]\n",
            "the stamp names bytes other than the ones written"
        );
    };

    note_ops::rewrite_links_in_file(
        &path,
        &target("Old note"),
        "New note",
        None,
        None,
        Some(&stamp),
    )
    .expect("the rewrite should land");

    let seen = stamped.into_inner();
    assert_eq!(seen.len(), 1, "one write, one stamp");
    assert_eq!(seen[0].0, path);
    assert_eq!(
        seen[0].1, "see [[Old note]]\n",
        "the stamp was made after the file had already been replaced"
    );
}

#[test]
fn the_reverse_rewrite_restores_the_file_byte_for_byte() {
    let text = "see [[ideas/Old note#Later|the one]], and [a](ideas/Old%20note.md)\r\n";
    let root = TempDir::new().expect("temp dir");
    let path = root.path().join("Linking note.md");
    std::fs::write(&path, text).expect("seed");
    let before = std::fs::read(&path).expect("read");

    let old = WikilinkTarget {
        name: "Old note".to_string(),
        folder: Some("ideas".to_string()),
        ..WikilinkTarget::default()
    };
    let new = WikilinkTarget {
        name: "New note".to_string(),
        folder: Some("ideas".to_string()),
        ..WikilinkTarget::default()
    };

    assert!(
        note_ops::rewrite_links_in_file(&path, &old, "New note", None, None, None)
            .expect("the rewrite should land")
    );
    assert_ne!(std::fs::read(&path).expect("read"), before);

    assert!(
        note_ops::rewrite_links_in_file(&path, &new, "Old note", None, None, None)
            .expect("the undo should land")
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        before,
        "undoing the rewrite did not restore the file"
    );
}
