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

/// Searches buffer titles and content for the user's query.
///
/// Raw input is converted to a sanitized prefix-match expression
/// ([`writ_core::search::to_prefix_match`]) so typing `tok` finds `token`
/// (search-as-you-type) and FTS5 operators in the input can never reach the
/// `MATCH` parser. A query with no usable token (empty, punctuation, or only
/// single characters) returns no results without touching the index.
#[tauri::command]
pub fn search_buffers(state: State<'_, AppState>, query: String) -> Result<Vec<String>, String> {
    let Some(match_query) = writ_core::search::to_prefix_match(&query) else {
        return Ok(Vec::new());
    };
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.search(&match_query).map_err(|e| e.to_string())
}
