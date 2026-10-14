//! The command usage IPC (ADR-048): runs add up in writ.db, a bad id refuses
//! the whole batch, and clear and prune reach the table.

use std::sync::Mutex as StdMutex;

use tempfile::TempDir;
use writ_core::command::usage::CommandUse;
use writ_tauri_lib::commands::command_usage::{
    clear_command_usage_inner, get_command_usage_inner, prune_command_usage_inner,
    record_command_uses_inner,
};
use writ_tauri_lib::state::AppState;

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

fn used(id: &str, count: u32, last_used_ms: u64) -> CommandUse {
    CommandUse {
        id: id.to_string(),
        count,
        last_used_ms,
    }
}

#[test]
fn recorded_runs_add_up_and_read_back() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let state = launch(data.path(), notes.path());

    record_command_uses_inner(&state, &[used("palette.open", 2, 10)]).expect("record");
    record_command_uses_inner(
        &state,
        &[used("palette.open", 1, 20), used("tab.new", 1, 5)],
    )
    .expect("record");

    let usage = get_command_usage_inner(&state).expect("usage");
    assert_eq!(usage["palette.open"].count, 3);
    assert_eq!(usage["palette.open"].last_used_ms, 20);
    assert_eq!(usage["tab.new"].count, 1);
    assert!(
        !data.path().join("config.toml").exists(),
        "running a command writes no config"
    );
}

#[test]
fn a_batch_with_a_bad_id_is_refused_whole() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let state = launch(data.path(), notes.path());

    let too_long = "x".repeat(257);
    for bad in ["", too_long.as_str()] {
        let refused = record_command_uses_inner(&state, &[used("tab.new", 1, 1), used(bad, 1, 1)]);
        assert!(refused.is_err(), "an id of {} bytes is refused", bad.len());
    }
    assert!(get_command_usage_inner(&state).expect("usage").is_empty());

    let longest = "y".repeat(256);
    record_command_uses_inner(&state, &[used(&longest, 1, 1)]).expect("256 bytes is accepted");
}

#[test]
fn prune_and_clear_reach_the_table() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let state = launch(data.path(), notes.path());
    record_command_uses_inner(&state, &[used("kept", 1, 1), used("gone", 1, 1)]).expect("record");

    prune_command_usage_inner(&state, vec!["kept".to_string()]).expect("prune");
    let usage = get_command_usage_inner(&state).expect("usage");
    assert_eq!(usage.keys().collect::<Vec<_>>(), vec!["kept"]);

    clear_command_usage_inner(&state).expect("clear");
    assert!(get_command_usage_inner(&state).expect("usage").is_empty());
}
