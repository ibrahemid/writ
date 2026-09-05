//! Rewriting the links that name a renamed note (spec L3).
//!
//! The pure rewrite is `writ_core::notes::rename`; what is asserted here is
//! the half that touches the disk: which files are refused before anything is
//! written, and that every write that does happen is stamped first.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_core::hash::sha256_bytes;
use writ_core::notes::guard::{DiskState, SF_DATALESS};
use writ_storage::errors::StorageError;
use writ_storage::note_ops::{self, LinkRewrite, RewriteTarget};
use writ_storage::notes_index::index_key;

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

/// The rename as one file's rewrite reads it.
fn renaming<'a>(target: &'a str, new_name: &'a str, all: &'a [String]) -> RewriteTarget<'a> {
    RewriteTarget {
        target,
        new_name,
        candidates: all,
        unindexed: &[],
    }
}

/// The renamed note's key, and the notes a link in `path` could reach: it and
/// the linking note. Neither has to exist on disk for a link to name it.
fn folder(path: &Path, note: &str) -> (String, Vec<String>) {
    let parent = path.parent().expect("a note sits in a folder");
    let renamed = index_key(&parent.join(format!("{note}.md")));
    let all = vec![renamed.clone(), index_key(path)];
    (renamed, all)
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
    let (target, all) = folder(&path, "Old note");

    let written = note_ops::rewrite_links_in_file(
        &path,
        &renaming(&target, "New note", &all),
        None,
        None,
        None,
    )
    .expect("the rewrite should land");

    assert_eq!(written, LinkRewrite::Written);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "see [[New note|the one]] for more\n"
    );
}

#[test]
fn a_file_naming_the_note_nowhere_is_not_written() {
    let (_root, path) = seeded("nothing links anywhere here\n");
    let (target, all) = folder(&path, "Old note");
    let before = recorded(&path);

    let written = note_ops::rewrite_links_in_file(
        &path,
        &renaming(&target, "New note", &all),
        None,
        None,
        None,
    )
    .expect("nothing to do is not a failure");

    assert_eq!(written, LinkRewrite::NoLink);
    assert_eq!(recorded(&path).hash, before.hash);
}

#[test]
fn a_file_that_is_not_downloaded_is_refused_before_it_is_read() {
    // The probe stands in for `SF_DATALESS`, so the refusal is asserted on
    // every platform rather than only where the flag can be set.
    let (_root, path) = seeded("see [[Old note]]\n");
    let (target, all) = folder(&path, "Old note");
    let probe = |_: &Path| Some(SF_DATALESS);

    let error = note_ops::rewrite_links_in_file(
        &path,
        &renaming(&target, "New note", &all),
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
    let (target, all) = folder(&path, "Old note");
    let last_known = recorded(&path);
    std::fs::write(&path, "somebody else wrote this, and [[Old note]]\n").expect("write");

    let error = note_ops::rewrite_links_in_file(
        &path,
        &renaming(&target, "New note", &all),
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
    let (target, all) = folder(&path, "Old note");
    let last_known = recorded(&path);

    let written = note_ops::rewrite_links_in_file(
        &path,
        &renaming(&target, "New note", &all),
        Some(last_known),
        None,
        None,
    )
    .expect("the guard should let this through");

    assert_eq!(written, LinkRewrite::Written);
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
    let (target, all) = folder(&path, "Old note");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");

    let error = note_ops::rewrite_links_in_file(
        &path,
        &renaming(&target, "New note", &all),
        None,
        None,
        None,
    )
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
    let (target, all) = folder(&path, "Old note");
    let stamped: std::cell::RefCell<Vec<(PathBuf, String)>> = std::cell::RefCell::new(Vec::new());
    let stamp = |file: &Path, bytes: &[u8]| {
        stamped.borrow_mut().push((
            file.to_path_buf(),
            std::fs::read_to_string(file).unwrap_or_default(),
        ));
        assert_eq!(
            String::from_utf8_lossy(bytes),
            "see [[New note]]\n",
            "the stamp names bytes other than the ones written"
        );
    };

    note_ops::rewrite_links_in_file(
        &path,
        &renaming(&target, "New note", &all),
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

    let ideas = root.path().join("ideas");
    std::fs::create_dir(&ideas).expect("folder");
    let old = index_key(&ideas.join("Old note.md"));
    let new = index_key(&ideas.join("New note.md"));
    let all = vec![old.clone(), new.clone(), index_key(&path)];

    assert_eq!(
        note_ops::rewrite_links_in_file(&path, &renaming(&old, "New note", &all), None, None, None)
            .expect("the rewrite should land"),
        LinkRewrite::Written
    );
    assert_ne!(std::fs::read(&path).expect("read"), before);

    assert_eq!(
        note_ops::rewrite_links_in_file(&path, &renaming(&new, "Old note", &all), None, None, None)
            .expect("the undo should land"),
        LinkRewrite::Written
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        before,
        "undoing the rewrite did not restore the file"
    );
}

/// A note the index has not heard of, sharing the renamed note's name: the
/// file is left exactly as it was and the caller is told which of the two
/// reasons it is.
#[test]
fn a_name_a_file_outside_the_candidate_list_answers_to_is_refused() {
    let (_root, path) = seeded("see [[Old note]]\n");
    let (target, all) = folder(&path, "Old note");
    let elsewhere = path
        .parent()
        .expect("folder")
        .join("archive")
        .join("Old note.md");

    let answer = note_ops::rewrite_links_in_file(
        &path,
        &RewriteTarget {
            target: &target,
            new_name: "New note",
            candidates: &all,
            unindexed: &[index_key(&elsewhere)],
        },
        None,
        None,
        None,
    )
    .expect("a name two notes answer to is not a failure");

    assert_eq!(
        answer,
        LinkRewrite::NameNotUnique(index_key(&elsewhere)),
        "the caller is told which file the name could also mean"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "see [[Old note]]\n"
    );
}

/// The walk behind that list: a note the index has not reached yet is one a
/// link can still mean.
#[test]
fn the_walk_finds_a_note_the_index_may_not_hold_yet() {
    let root = TempDir::new().expect("temp dir");
    let folder = root.path().join("archive");
    std::fs::create_dir(&folder).expect("folder");
    std::fs::write(folder.join("Note.md"), "written a moment ago\n").expect("write");
    std::fs::write(root.path().join("Other.md"), "not this one\n").expect("write");

    let found = note_ops::files_named(root.path(), &["note".to_string()]);

    assert_eq!(found, vec![folder.join("Note.md")]);
}

/// The walk is the index's walk, so a folder the index prunes holds nothing a
/// link reaches, and a name a link means is never taken from a file that is
/// not note text.
#[test]
fn the_walk_passes_over_what_the_index_would_never_hold() {
    let root = TempDir::new().expect("temp dir");
    for folder in [".git", ".obsidian", "node_modules"] {
        std::fs::create_dir(root.path().join(folder)).expect("folder");
    }
    std::fs::write(root.path().join(".git").join("index"), [0u8, 1, 2, 3, 4]).expect("write");
    std::fs::write(
        root.path().join(".obsidian").join("index.md"),
        "a key map\n",
    )
    .expect("write");
    std::fs::write(
        root.path().join("node_modules").join("index.md"),
        "a package\n",
    )
    .expect("write");
    std::fs::write(root.path().join("index.png"), [137u8, 80, 78, 71]).expect("write");

    let found = note_ops::files_named(root.path(), &["index".to_string()]);

    assert!(found.is_empty(), "{found:?}");
}

/// A file that is not note text is not a note a link could mean, wherever it
/// sits. This is the kind test rather than the pruning: nothing prunes `bin/`.
#[test]
fn the_walk_passes_over_a_file_that_is_not_note_text() {
    let root = TempDir::new().expect("temp dir");
    let folder = root.path().join("bin");
    std::fs::create_dir(&folder).expect("folder");
    std::fs::write(folder.join("Note"), [0u8, 159, 146, 150, 0, 1]).expect("write");

    let found = note_ops::files_named(root.path(), &["note".to_string()]);

    assert!(found.is_empty(), "{found:?}");
}

/// A symlink is taken at the name it carries, because its target is outside
/// the folder this walk was pointed at and reading it is a read of a file
/// nobody named. An extension a note never has is not a note here, whatever
/// the bytes behind it turn out to be.
#[cfg(unix)]
#[test]
fn the_walk_takes_a_symlink_at_its_name() {
    let root = TempDir::new().expect("temp dir");
    let outside = TempDir::new().expect("temp dir");
    let real = outside.path().join("Note");
    std::fs::write(&real, "plain text under a name that says nothing\n").expect("write");
    std::os::unix::fs::symlink(&real, root.path().join("Note")).expect("symlink");

    let found = note_ops::files_named(root.path(), &["note".to_string()]);

    assert!(found.is_empty(), "{found:?}");
}

/// A symlinked note answers to its own name, and the walk that will not
/// descend into a symlinked folder still sees it.
#[cfg(unix)]
#[test]
fn the_walk_finds_a_note_behind_a_symlink() {
    let root = TempDir::new().expect("temp dir");
    let outside = TempDir::new().expect("temp dir");
    let real = outside.path().join("Note.md");
    std::fs::write(&real, "the note itself\n").expect("write");
    let link = root.path().join("Note.md");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    let found = note_ops::files_named(root.path(), &["note".to_string()]);

    assert_eq!(found, vec![link]);
}
