use crate::state::AppState;
use serde::Serialize;
use tauri::State;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::search::SearchHit;

/// Caps the number of ranked hits returned to the UI; `total` still reports the
/// full match count so the footer can show "N of M".
const SEARCH_RESULT_LIMIT: usize = 100;

/// Full-text search results for the sidebar: the top-ranked hits plus the total
/// number of matches (which may exceed `hits.len()`).
#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub total: usize,
}

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
pub fn search_buffers(state: State<'_, AppState>, query: String) -> Result<SearchResults, String> {
    let Some(match_query) = writ_core::search::to_prefix_match(&query) else {
        return Ok(SearchResults {
            hits: Vec::new(),
            total: 0,
        });
    };
    let terms = writ_core::search::search_terms(&query);
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let hits = store
        .search_hits(&match_query, &terms, SEARCH_RESULT_LIMIT)
        .map_err(|e| e.to_string())?;
    let total = store.search_count(&match_query).map_err(|e| e.to_string())?;
    Ok(SearchResults { hits, total })
}
