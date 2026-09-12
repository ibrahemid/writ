//! An edit to config.toml from outside Writ reaches the running app:
//! `AppState::reload_config_from_disk` replaces the copy every command
//! answers from, and leaves it alone when the file no longer parses.

use std::sync::Mutex as StdMutex;

use tempfile::TempDir;
use writ_tauri_lib::state::AppState;

// `AppState::initialize` reads `WRIT_DATA_DIR` and `WRIT_NOTES_DIR`; the tests
// in this file all set them, so they take turns.
static ENV_LOCK: StdMutex<()> = StdMutex::new(());

fn launch(data_dir: &std::path::Path, notes_dir: &std::path::Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::set_var("WRIT_NOTES_DIR", notes_dir);
    let state = AppState::initialize().expect("app state");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    state
}

fn config_text(state: &AppState) -> String {
    let config = state.config.lock().expect("config lock");
    format!(
        "{}|{}|{:?}",
        config.sidebar.width,
        config.editor.font_size,
        config.sidebar.collapsed
    )
}

#[test]
fn an_external_edit_replaces_the_running_config() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let state = launch(data.path(), notes.path());
    assert_eq!(config_text(&state), "240|16|[]");

    // What another program, or a hand in an editor, leaves behind: a file
    // that names only what it changed.
    let path = data.path().join("config.toml");
    std::fs::write(&path, "[sidebar]\nwidth = 300\ncollapsed = [\"tags\"]\n").expect("edit config");

    assert!(state.reload_config_from_disk());
    assert_eq!(
        config_text(&state),
        "300|16|[Tags]",
        "the copy commands answer from is the file's"
    );
}

#[test]
fn a_file_that_no_longer_parses_leaves_the_running_config_alone() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let state = launch(data.path(), notes.path());

    let path = data.path().join("config.toml");
    std::fs::write(&path, "[sidebar]\nwidth = \"wide\"\n").expect("break config");

    assert!(!state.reload_config_from_disk());
    assert_eq!(config_text(&state), "240|16|[]");
}
