//! Text a closed tab handed over, carried across a reopen of its note, a save
//! that lands and an exit that ran nothing on the way out.
//!
//! The file is read-only, so the save is refused, and Cmd+W closes the tab
//! without asking because that refusal is not retried: the frontend hands the
//! text over as the tab goes. Later in the session the file takes writes
//! again, the person opens the note, which shows its file, types, and the save
//! lands. The text typed before the close is in no file at that point.
//!
//! One test in its own binary, because `initialize` reads `WRIT_DATA_DIR` and
//! `WRIT_NOTES_DIR` from the process environment and a second test in the same
//! binary would race it.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_tauri_lib::commands::buffer::{
    close_buffer_inner, record_unsaved_notes_inner, save_buffer_content_inner, UnsavedNote,
    ERR_READ_ONLY_DESTINATION,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::security::canonicalize_for_authorization;
use writ_tauri_lib::state::AppState;

const ON_DISK: &str = "what the file held\n";
const TYPED_BEFORE_THE_CLOSE: &str = "what the file held\ntyped before the close\n";
const TYPED_AFTER_THE_REOPEN: &str = "what the file held\ntyped after the reopen\n";

fn recovered_copies(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains("(recovered "))
        })
        .collect();
    found.sort();
    found
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).expect("set permissions");
}

fn open(state: &AppState, canonical: &str) -> String {
    state
        .authorized_paths
        .record_for_open(canonical.to_string());
    open_file_from_path(state, canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id
}

#[test]
fn text_a_closed_tab_handed_over_survives_a_reopen_a_landed_save_and_a_relaunch() {
    let data = TempDir::new().expect("data folder");
    let notes = TempDir::new().expect("notes folder");
    std::env::set_var("WRIT_DATA_DIR", data.path());
    std::env::set_var("WRIT_NOTES_DIR", notes.path());

    let session = AppState::initialize().expect("the first launch");
    let note = session.notes_root().join("Shared.md");
    std::fs::write(&note, ON_DISK).expect("write the note");
    let canonical = canonicalize_for_authorization(&note).expect("canonical");
    let id = open(&session, &canonical);

    set_read_only(&note, true);
    let refused = save_buffer_content_inner(&session, &id, TYPED_BEFORE_THE_CLOSE)
        .expect_err("a read-only file refuses the save");
    assert!(refused.starts_with(ERR_READ_ONLY_DESTINATION), "{refused}");
    close_buffer_inner(&session, &id).expect("close the tab");
    record_unsaved_notes_inner(
        &session,
        vec![UnsavedNote {
            id: id.clone(),
            content: TYPED_BEFORE_THE_CLOSE.to_string(),
        }],
    )
    .expect("hand the text over");

    set_read_only(&note, false);
    assert_eq!(
        open(&session, &canonical),
        id,
        "the reopen brings the row back"
    );
    save_buffer_content_inner(&session, &id, TYPED_AFTER_THE_REOPEN).expect("the save lands");
    drop(session);

    let relaunched = AppState::initialize();
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    let relaunched = relaunched.expect("the relaunch");

    let offered = relaunched
        .recovered_buffers
        .lock()
        .expect("recovered buffers")
        .iter()
        .find(|buf| buf.id == id)
        .cloned()
        .expect("the relaunch offers the text typed before the close");
    assert!(!offered.removed_on_disk);
    assert_eq!(offered.content, TYPED_BEFORE_THE_CLOSE);

    assert_eq!(
        std::fs::read_to_string(&note).expect("read the note"),
        TYPED_AFTER_THE_REOPEN,
        "the file keeps the save that landed"
    );
    let copies = recovered_copies(&relaunched.notes_root());
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(
        std::fs::read_to_string(&copies[0]).expect("read the copy"),
        TYPED_BEFORE_THE_CLOSE,
        "the text typed before the close is on disk beside the note"
    );
    assert_eq!(
        relaunched
            .store
            .lock()
            .expect("store")
            .unsaved_text_count()
            .expect("count the journal"),
        0
    );
}
