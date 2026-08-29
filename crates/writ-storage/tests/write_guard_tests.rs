//! The write guard at the layer that touches the disk (ADR-028 §5).

use std::path::Path;

use chrono::{DateTime, Utc};
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::hash::{sha256_bytes, sha256_hex};
use writ_core::notes::guard::DiskState;
use writ_storage::buffer_store::{write_conflict_copy, BufferStore};
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

    let result = store.save_to_source("guard-1", "# What the user typed", Some(last_known));

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

    let result = store.save_to_source("guard-1", "# What the user typed", Some(last_known));
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

    let result = store.save_to_source("guard-1", "# What the user typed", Some(last_known));

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
        .save_to_source("guard-1", "# The same edit, made twice", Some(last_known))
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
        .save_to_source("guard-1", "# What the user typed", Some(last_known))
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

    let written = write_conflict_copy(&path, "mine", fixed_now()).expect("copy");

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

    let first = write_conflict_copy(&path, "first", fixed_now()).expect("first copy");
    let second = write_conflict_copy(&path, "second", fixed_now()).expect("second copy");

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
