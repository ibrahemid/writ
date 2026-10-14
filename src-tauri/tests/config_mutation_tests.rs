//! Two changes to the config made at the same time both land, in the running
//! copy and in config.toml (TS-29).

use std::sync::{Arc, Barrier, Mutex as StdMutex};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_tauri_lib::commands::first_run::dismiss_first_run_hint_inner;
use writ_tauri_lib::commands::spelling::add_ignored_word_to_state;
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

const ROUNDS: usize = 40;

#[test]
fn two_concurrent_mutations_both_land() {
    let data = TempDir::new().expect("data dir");
    let notes = TempDir::new().expect("notes dir");
    let state = Arc::new(launch(data.path(), notes.path()));

    for round in 0..ROUNDS {
        let barrier = Arc::new(Barrier::new(2));
        let first = {
            let (state, barrier) = (Arc::clone(&state), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                if round == 0 {
                    dismiss_first_run_hint_inner(&state).expect("dismiss");
                } else {
                    add_ignored_word_to_state(&state, format!("alpha{round}")).expect("add");
                }
            })
        };
        let second = {
            let (state, barrier) = (Arc::clone(&state), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                add_ignored_word_to_state(&state, format!("beta{round}")).expect("add");
            })
        };
        first.join().expect("first thread");
        second.join().expect("second thread");
    }

    let path = data.path().join("config.toml");
    let on_disk = WritConfig::parse(&std::fs::read_to_string(path).expect("config written"))
        .expect("config parses");
    let in_memory = state.config.lock().expect("config lock").clone();
    for config in [&in_memory, &on_disk] {
        assert!(config.first_run.hint_dismissed);
        for round in 0..ROUNDS {
            let beta = format!("beta{round}");
            assert!(config.spelling.ignored_words.contains(&beta), "{beta} lost");
            if round > 0 {
                let alpha = format!("alpha{round}");
                assert!(
                    config.spelling.ignored_words.contains(&alpha),
                    "{alpha} lost"
                );
            }
        }
    }
}
