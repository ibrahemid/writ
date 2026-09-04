use crate::poison::recover_poison;
use crate::state::AppState;
use std::time::Instant;
use tauri::State;
use writ_core::config::WritConfig;

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<WritConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

/// Serializes and writes `config` to disk, recording the write in the
/// watcher ignore set so the change is not re-surfaced as external.
///
/// The key is the config file's canonical path under the config namespace,
/// which is what the config watcher looks up. The bare `config.toml` it used
/// to be was shared with every note of that name (ADR-028 section 6).
pub(crate) fn persist_config(state: &AppState, config: &WritConfig) -> Result<(), String> {
    let contents = state
        .config_store
        .serialize(config)
        .map_err(|e| e.to_string())?;

    let key = writ_core::watcher::ignore::config_key(&crate::watcher::handler::ignore_key_path(
        state.config_store.path(),
    ));

    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::config::persist_config",
        );
        ignore.record(key, contents.as_bytes(), Instant::now());
    }

    state
        .config_store
        .write_serialized(&contents)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_config(state: State<'_, AppState>, config: WritConfig) -> Result<(), String> {
    persist_config(&state, &config)?;

    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    *current = config;
    Ok(())
}
