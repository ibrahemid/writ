//! Which layer turns down a data folder that overlaps the notes folder.
//!
//! Its own binary: `AppState::initialize` reads `WRIT_DATA_DIR` and
//! `WRIT_NOTES_DIR` from the process environment, so a test that sets them
//! cannot share a process with another that does.

use tempfile::TempDir;
use writ_tauri_lib::state::{AppState, NotesRootFallbackReason};

/// `writ_core::notes::refuse_notes_root` carries the same overlap rule as the
/// data-folder verdict and runs first, at notes-root resolution, so the launch
/// falls back to `<data folder>/Writ` instead of stopping. The verdict in
/// `AppState::initialize` is the invariant behind that rule rather than the
/// rule itself, which is why the launch below succeeds.
#[test]
fn the_notes_root_rule_diverts_a_data_folder_inside_the_notes_folder() {
    let root = TempDir::new().expect("temp dir");
    let notes = root.path().join("notes");
    let data_dir = notes.join(".writ");
    std::fs::create_dir_all(&data_dir).expect("data dir");

    std::env::set_var("WRIT_DATA_DIR", &data_dir);
    std::env::set_var("WRIT_NOTES_DIR", &notes);
    let state = AppState::initialize().expect("the notes root rule diverts before the guard");
    std::env::remove_var("WRIT_NOTES_DIR");
    std::env::remove_var("WRIT_DATA_DIR");

    assert_eq!(
        state.notes_root(),
        writ_tauri_lib::security::canonicalize_root(data_dir.join("Writ")).expect("notes folder")
    );
    assert_eq!(
        state.notes_root_fallback().map(|fallback| fallback.reason),
        Some(NotesRootFallbackReason::HoldsWritData)
    );
}
