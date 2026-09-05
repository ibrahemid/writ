//! The relaunch after an unclean shutdown, driven through `AppState::initialize`.
//!
//! The storage tests drive `restore_recovered_content` directly; this one
//! drives the launch, because what the launch does with the outcome is its own
//! behaviour: a note whose file moved on while Writ was down must come back
//! with its file intact and the snapshot beside it, and every recovered note
//! must leave the launch with the write guard seeded, or the first save after
//! a crash is the unguarded one. A note whose file was deleted must come back
//! with no file written at all: the tab is handed the text and shows it as
//! removed on disk (ADR-033 decision 15).
//!
//! One test in its own binary, because `initialize` reads `WRIT_DATA_DIR` and
//! `WRIT_NOTES_DIR` from the process environment and a second test in the same
//! binary would race it. Both cases therefore ride one launch, which is also
//! how they arrive in life.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::hash::sha256_bytes;
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_tauri_lib::state::AppState;

fn row(id: &str, title: &str, source_path: &Path) -> BufferDocument {
    let now = Utc::now() - chrono::Duration::seconds(30);
    BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
        filename: format!("{id}.txt"),
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

fn dated_copies(dir: &Path, label: &str) -> Vec<PathBuf> {
    let marker = format!("({label} ");
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains(&marker))
        })
        .collect();
    found.sort();
    found
}

/// Leaves the data folder as a crash would: two notes with files, and a dirty
/// snapshot newer than both rows holding what the editor had at the time.
fn seed_a_crashed_session(writ_dir: &Path, notes_dir: &Path) {
    std::fs::create_dir_all(writ_dir.join("buffers")).expect("mirror folder");

    let moved_on = notes_dir.join("Shared.md");
    let steady = notes_dir.join("Steady.md");
    // The third note's file is not written at all: this is the tab whose file
    // the person deleted before the crash.
    let deleted = notes_dir.join("Deleted.md");
    std::fs::write(&moved_on, "what a sync client delivered").unwrap();
    std::fs::write(&steady, "steady text").unwrap();

    let db_path = writ_dir.join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let store = BufferStore::new(conn, writ_dir.join("buffers"));
    store.insert(&row("moved-on", "Shared", &moved_on)).unwrap();
    store.insert(&row("steady", "Steady", &steady)).unwrap();
    store.insert(&row("deleted", "Deleted", &deleted)).unwrap();

    // The snapshot is written now and the rows are stamped half a minute back,
    // so it resolves as newer without the test having to sleep through
    // SQLite's second-granularity clock. `is_clean = false` is the crash.
    let snapshot: HashMap<String, String> = HashMap::from([
        (
            "moved-on".to_string(),
            "what the crash was holding".to_string(),
        ),
        ("steady".to_string(), "steady text".to_string()),
        (
            "deleted".to_string(),
            "the line typed after the file went".to_string(),
        ),
    ]);
    store
        .write_session_snapshot(&snapshot, false)
        .expect("write the dirty snapshot");
    assert!(store.is_dirty_shutdown().expect("dirty check"));
}

#[test]
fn an_unclean_relaunch_keeps_what_arrived_while_writ_was_down_and_seeds_the_guard() {
    let data = TempDir::new().expect("data folder");
    let notes = TempDir::new().expect("notes folder");
    seed_a_crashed_session(data.path(), notes.path());

    std::env::set_var("WRIT_DATA_DIR", data.path());
    std::env::set_var("WRIT_NOTES_DIR", notes.path());
    let state = AppState::initialize().expect("the launch must not fail");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");

    assert!(
        state.was_dirty_shutdown,
        "the launch has to see the crash for any of this to run"
    );

    // The note whose file moved on: the version that arrived survives, and the
    // text the crash was holding is beside it rather than lost or written over.
    let moved_on = notes.path().join("Shared.md");
    assert_eq!(
        std::fs::read_to_string(&moved_on).unwrap(),
        "what a sync client delivered",
        "a relaunch must not write a pre-crash snapshot over a newer version"
    );
    let copies = dated_copies(notes.path(), "recovered");
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(
        copies[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Shared (recovered "),
        "{:?}",
        copies[0]
    );
    assert_eq!(
        std::fs::read_to_string(&copies[0]).unwrap(),
        "what the crash was holding"
    );

    // The guard is seeded from what is on disk, not from the snapshot, so the
    // first save after the relaunch is measured against the file.
    assert_eq!(
        state.disk_state("moved-on").map(|state| state.hash),
        Some(sha256_bytes(b"what a sync client delivered")),
        "the record has to describe the file, not the text that was set aside"
    );

    // The note nothing touched: restored, and its record seeded too.
    let steady = notes.path().join("Steady.md");
    assert_eq!(std::fs::read_to_string(&steady).unwrap(), "steady text");
    assert_eq!(
        state.disk_state("steady").map(|state| state.hash),
        Some(sha256_bytes(b"steady text")),
        "a note the crash did not disturb still leaves the launch guarded"
    );

    // The note whose file was deleted: the launch writes nothing, leaves
    // nothing beside the path, and hands the text to the tab.
    let deleted = notes.path().join("Deleted.md");
    assert!(
        !deleted.exists(),
        "a relaunch must not put back a file somebody deleted"
    );
    assert!(
        dated_copies(notes.path(), "recovered")
            .iter()
            .all(|copy| !copy.to_string_lossy().contains("Deleted")),
        "a copy beside a path somebody cleared is a file they did not ask for"
    );
    assert_eq!(
        state.disk_state("deleted").map(|state| state.hash),
        None,
        "there is no file to describe, so nothing is recorded about one"
    );
    let handed_over = state
        .recovered_buffers
        .lock()
        .expect("recovered buffers")
        .iter()
        .find(|buf| buf.id == "deleted")
        .cloned()
        .expect("the text has to reach the frontend, since nothing else holds it");
    assert!(
        handed_over.removed_on_disk,
        "the tab has to come up removed on disk, not blank"
    );
    assert_eq!(handed_over.content, "the line typed after the file went");
}
