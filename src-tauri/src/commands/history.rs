use crate::state::AppState;
use tauri::State;
use writ_core::buffer::document::{BufferDocument, BufferStatus};

#[tauri::command]
pub fn list_history(state: State<'_, AppState>) -> Result<Vec<BufferDocument>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .list_by_status(BufferStatus::History)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.restore(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let history = store
        .list_by_status(BufferStatus::History)
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = history.into_iter().map(|buf| buf.id).collect();
    store.delete_many(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search_buffers(state: State<'_, AppState>, query: String) -> Result<Vec<String>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.search(&query).map_err(|e| e.to_string())
}
