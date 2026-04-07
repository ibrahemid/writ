use crate::state::AppState;
use tauri::State;
use writ_core::config::WritConfig;

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<WritConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

#[tauri::command]
pub fn update_config(state: State<'_, AppState>, config: WritConfig) -> Result<(), String> {
    state
        .config_store
        .write(&config)
        .map_err(|e| e.to_string())?;
    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    *current = config;
    Ok(())
}
