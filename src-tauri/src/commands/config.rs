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
pub(crate) fn persist_config(state: &AppState, config: &WritConfig) -> Result<(), String> {
    let contents = state
        .config_store
        .serialize(config)
        .map_err(|e| e.to_string())?;

    let filename = state
        .config_store
        .path()
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| "config path has no file name".to_string())?;

    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::config::persist_config",
        );
        ignore.record(filename, contents.as_bytes(), Instant::now());
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
