//! A launch whose config.toml holds a value Writ cannot read starts on the
//! rest of the file, names the value, and writes nothing until the file reads
//! in full again (ADR-048), apart from a moved folder's path. Command usage a
//! file written before ADR-048 carries moves into the database once.

use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_tauri_lib::commands::command_usage::{clear_command_usage_inner, get_command_usage_inner};
use writ_tauri_lib::commands::config::{config_diagnostics, update_config_inner};
use writ_tauri_lib::commands::notes::move_notes_folder_to;
use writ_tauri_lib::state::AppState;

// `AppState::initialize` reads `WRIT_DATA_DIR` and `WRIT_NOTES_DIR`; the tests
// in this file all set them, so they take turns.
static ENV_LOCK: StdMutex<()> = StdMutex::new(());

fn launch(data_dir: &std::path::Path, notes_dir: &std::path::Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::set_var("WRIT_NOTES_DIR", notes_dir);
    let state = AppState::initialize();
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    state.expect("a bad value in config.toml must not stop the launch")
}

/// A launch whose notes folder comes from config.toml, as on a person's
/// machine: `WRIT_NOTES_DIR` would win over the file.
fn launch_on_configured_root(data_dir: &Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::remove_var("WRIT_NOTES_DIR");
    let state = AppState::initialize();
    std::env::remove_var("WRIT_DATA_DIR");
    state.expect("a bad value in config.toml must not stop the launch")
}

/// A config holding one value Writ cannot read, with the notes folder at
/// `root`. A literal string, so a Windows path needs no escaping.
fn held_with_root(root: &Path) -> String {
    format!(
        "# mine\n[editor]\nfont_size = \"16\"\n\n[notes]\nroot = '{}'\n",
        root.display()
    )
}

/// Everything in [`held_with_root`] before the folder's path.
const HELD_BEFORE_ROOT: &str = "# mine\n[editor]\nfont_size = \"16\"\n\n[notes]\nroot = ";

fn live(state: &AppState) -> WritConfig {
    state.config.lock().expect("config lock").clone()
}

const BAD: &str = "[editor]\nfont_size = \"16\"\ntab_size = 4\n\n[sidebar]\nwidth = 300\n";

#[test]
fn an_invalid_value_starts_on_the_rest_of_the_file_and_names_it() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let path = data.path().join("config.toml");
    std::fs::write(&path, BAD).expect("write config");

    let state = launch(data.path(), notes.path());

    let config = live(&state);
    assert_eq!(config.editor.font_size, 16, "the default stands in");
    assert_eq!(config.editor.tab_size, 4);
    assert_eq!(config.sidebar.width, 300);
    let named = config_diagnostics(&state);
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].key, "editor.font_size");
    assert_eq!(named[0].line, 2);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), BAD);
}

#[test]
fn nothing_is_written_until_the_file_reads_in_full() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let path = data.path().join("config.toml");
    std::fs::write(&path, BAD).expect("write config");
    let state = launch(data.path(), notes.path());

    // A settings change lasts the session and leaves the file alone.
    let mut changed = live(&state);
    changed.sidebar.width = 280;
    update_config_inner(&state, changed).expect("the change is taken");
    assert_eq!(live(&state).sidebar.width, 280);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), BAD);

    // Fixed by hand, the file is read again and writes resume.
    std::fs::write(&path, "[editor]\nfont_size = 16\ntab_size = 4\n").expect("fix config");
    assert!(state.reload_config_from_disk());
    assert!(config_diagnostics(&state).is_empty());
    let mut changed = live(&state);
    changed.sidebar.width = 260;
    update_config_inner(&state, changed).expect("write");
    let on_disk = WritConfig::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(on_disk.sidebar.width, 260);
    assert_eq!(on_disk.editor.tab_size, 4);
}

#[test]
fn command_usage_in_an_older_config_moves_into_the_database_once() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let path = data.path().join("config.toml");
    let text = "[commands.usage.\"palette.open\"]\ncount = 17\nlast_used_ms = 1715500000000\n";
    std::fs::write(&path, text).expect("write config");

    let state = launch(data.path(), notes.path());
    let usage = get_command_usage_inner(&state).expect("usage");
    assert_eq!(usage["palette.open"].count, 17);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "the move reads only"
    );

    clear_command_usage_inner(&state).expect("clear");
    drop(state);
    let relaunched = launch(data.path(), notes.path());
    assert!(
        get_command_usage_inner(&relaunched)
            .expect("usage")
            .is_empty(),
        "a later launch does not bring cleared usage back"
    );
}

#[test]
fn a_folder_moved_while_values_are_unread_reaches_the_file_and_the_next_launch() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let elsewhere = TempDir::new().expect("destination parent");
    let path = data.path().join("config.toml");
    std::fs::write(&path, held_with_root(notes.path())).expect("write config");
    let state = launch_on_configured_root(data.path());
    std::fs::write(state.notes_root().join("Plan.md"), "mine").expect("seed");

    let outcome =
        move_notes_folder_to(&state, &elsewhere.path().join("Moved")).expect("the move runs");
    assert_eq!(outcome.moved, 1);

    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.starts_with(HELD_BEFORE_ROOT),
        "the value to fix and the comment keep their bytes: {on_disk}"
    );
    let read = WritConfig::parse_lenient(&on_disk);
    assert_eq!(
        read.config.notes.root.as_deref(),
        Some(outcome.new_root.as_str())
    );
    assert_eq!(read.diagnostics[0].key, "editor.font_size");
    assert_eq!(config_diagnostics(&state)[0].key, "editor.font_size");

    drop(state);
    let relaunched = launch_on_configured_root(data.path());
    assert_eq!(relaunched.notes_root(), PathBuf::from(&outcome.new_root));
    assert!(relaunched.notes_root().join("Plan.md").exists());
}

#[test]
fn fixing_the_value_after_a_move_keeps_the_folder_the_files_went_to() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let elsewhere = TempDir::new().expect("destination parent");
    let path = data.path().join("config.toml");
    std::fs::write(&path, held_with_root(notes.path())).expect("write config");
    let state = launch_on_configured_root(data.path());
    let outcome =
        move_notes_folder_to(&state, &elsewhere.path().join("Moved")).expect("the move runs");

    let fixed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("font_size = \"16\"", "font_size = 16");
    std::fs::write(&path, fixed).expect("fix config");
    assert!(state.reload_config_from_disk());
    assert!(config_diagnostics(&state).is_empty());
    assert_eq!(
        live(&state).notes.root.as_deref(),
        Some(outcome.new_root.as_str())
    );

    let mut changed = live(&state);
    changed.sidebar.width = 260;
    update_config_inner(&state, changed).expect("write");
    let on_disk = WritConfig::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        on_disk.notes.root.as_deref(),
        Some(outcome.new_root.as_str())
    );
    assert_eq!(on_disk.sidebar.width, 260);
}

#[test]
fn a_move_the_file_cannot_record_is_turned_down_before_anything_moves() {
    for text in ["[editor\nfont_size = 16\n", "notes = 5\n"] {
        let data = TempDir::new().expect("data dir");
        let notes = TempDir::new().expect("notes dir");
        let elsewhere = TempDir::new().expect("destination parent");
        let path = data.path().join("config.toml");
        std::fs::write(&path, text).expect("write config");
        let state = launch(data.path(), notes.path());
        assert!(!config_diagnostics(&state).is_empty(), "{text}");
        let from = state.notes_root();
        std::fs::write(from.join("Plan.md"), "mine").expect("seed");
        let destination = elsewhere.path().join("Moved");

        let error = move_notes_folder_to(&state, &destination).expect_err("turned down");

        assert!(error.contains("config.toml"), "{error}");
        assert!(from.join("Plan.md").exists(), "{text}: nothing moved");
        assert!(!destination.exists(), "{text}: no folder is left behind");
        assert_eq!(state.notes_root(), from);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}
