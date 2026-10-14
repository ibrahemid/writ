//! A new note whose first file the notes folder would not take, carried across
//! exits that ran nothing on the way out.
//!
//! The first save of a new note names its file and writes it. When the write
//! makes nothing, the text is journaled and the note is left with no file, so
//! a relaunch gives it one rather than reading the name as a file somebody
//! deleted. A relaunch that cannot make the file either leaves the note and
//! its text for the launch after.
//!
//! One test in its own binary, because `initialize` reads `WRIT_DATA_DIR` and
//! `WRIT_NOTES_DIR` from the process environment and a second test in the same
//! binary would race it.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::heartbeat;
use writ_tauri_lib::state::AppState;

const TYPED: &str = "the first words of a new note\n";

/// Locks a folder against new files (`0o555`) or unlocks it (`0o755`).
fn set_folder_mode(folder: &Path, mode: u32) {
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode))
        .expect("set the folder's mode");
}

/// What the journal holds for `id`, if anything.
fn journaled(state: &AppState, id: &str) -> Option<String> {
    state
        .store
        .lock()
        .expect("store")
        .unsaved_text()
        .expect("read the journal")
        .entries
        .into_iter()
        .find(|entry| entry.buffer_id == id)
        .map(|entry| entry.text)
}

/// A note with no file yet, the way a new tab starts (`create_buffer`).
fn new_note() -> BufferDocument {
    let now = chrono::Utc::now();
    BufferDocument {
        id: "fresh".to_string(),
        title: "Fresh".to_string(),
        filename: "fresh.txt".to_string(),
        status: BufferStatus::Active,
        language: None,
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
        encoding: None,
    }
}

#[test]
fn a_new_note_whose_first_file_was_never_made_gets_one_at_a_later_launch() {
    let data = TempDir::new().expect("data folder");
    let notes = TempDir::new().expect("notes folder");
    std::env::set_var("WRIT_DATA_DIR", data.path());
    std::env::set_var("WRIT_NOTES_DIR", notes.path());

    let session = AppState::initialize().expect("the first launch");
    let notes_root = session.notes_root();
    session
        .store
        .lock()
        .expect("store")
        .insert(&new_note())
        .expect("a new tab");
    set_folder_mode(&notes_root, 0o555);
    let refused = save_buffer_content_inner(&session, "fresh", TYPED);
    set_folder_mode(&notes_root, 0o755);
    refused.expect_err("the notes folder took no new file");
    heartbeat::beat(&session);
    drop(session);

    set_folder_mode(&notes_root, 0o555);
    let second = AppState::initialize();
    set_folder_mode(&notes_root, 0o755);
    let second = second.expect("the second launch");
    let handed = second
        .recovered_buffers
        .lock()
        .expect("recovered buffers")
        .iter()
        .find(|buf| buf.id == "fresh")
        .cloned()
        .expect("the tab is handed the text");
    assert!(
        handed.removed_on_disk,
        "the tab came up without the bar that holds its saves"
    );
    assert_eq!(handed.content, TYPED);
    assert_eq!(
        journaled(&second, "fresh").as_deref(),
        Some(TYPED),
        "the text waits for the next launch"
    );
    assert!(
        second.store.lock().expect("store").get("fresh").is_ok(),
        "the note is still there"
    );
    heartbeat::beat(&second);
    drop(second);

    let third = AppState::initialize();
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    let third = third.expect("the third launch");

    let offered = third
        .recovered_buffers
        .lock()
        .expect("recovered buffers")
        .iter()
        .find(|buf| buf.id == "fresh")
        .cloned()
        .expect("the launch restores the text");
    assert!(
        !offered.removed_on_disk,
        "a note whose first file was never made came back as deleted"
    );
    assert_eq!(offered.content, TYPED);
    let path = third
        .store
        .lock()
        .expect("store")
        .get("fresh")
        .expect("the note's row")
        .source_path
        .expect("the launch gave the note its file");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the note"),
        TYPED
    );
    assert_eq!(Path::new(&path).parent(), Some(notes_root.as_path()));
    assert_eq!(journaled(&third, "fresh"), None);
}
