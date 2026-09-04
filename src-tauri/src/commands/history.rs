use crate::state::AppState;
use serde::Serialize;
use tauri::State;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::search::SearchHit;
use writ_core::workspace::file_search::FileHit;

/// Caps the number of ranked hits returned to the UI; `total` still reports the
/// full match count so the footer can show "N of M".
const SEARCH_RESULT_LIMIT: usize = 100;

/// Caps the quick-open list. A name palette the user scrolls is a name palette
/// they should have typed more into.
const QUICK_OPEN_LIMIT: usize = 50;

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

/// IPC: brings a closed note back as an open tab.
///
/// The tab starts being followed again here. Its watch went when it closed,
/// and the file has had every moment since then to change.
#[tauri::command]
pub fn restore_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let doc = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.restore(&id).map_err(|e| e.to_string())?;
        store.get(&id).map_err(|e| e.to_string())?
    };
    state.follow_note_file(&doc);
    Ok(())
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

/// Searches note names and content for the user's query.
///
/// Raw input is converted to a sanitized prefix-match expression
/// ([`writ_core::search::to_prefix_match`]) so typing `tok` finds `token`
/// (search-as-you-type) and FTS5 operators in the input can never reach the
/// `MATCH` parser. A query with no usable token (empty, punctuation, or only
/// single characters) returns no results without touching the index.
///
/// The command name and payload are unchanged; what moved underneath is the
/// index, which is keyed by file path now (ADR-028 section 7). A hit carries
/// the note's path, plus the id of the tab already showing it when there is
/// one, so the sidebar can focus an open tab and open everything else by path.
#[tauri::command]
pub fn search_buffers(state: State<'_, AppState>, query: String) -> Result<SearchResults, String> {
    let Some(match_query) = writ_core::search::to_prefix_match(&query) else {
        return Ok(SearchResults {
            hits: Vec::new(),
            total: 0,
        });
    };
    let terms = writ_core::search::search_terms(&query);
    let mut hits = state
        .notes_index
        .search_hits(&match_query, &terms, SEARCH_RESULT_LIMIT)
        .map_err(|e| e.to_string())?;
    let total = state
        .notes_index
        .count(&match_query)
        .map_err(|e| e.to_string())?;

    let store = state.store.lock().map_err(|e| e.to_string())?;
    for hit in &mut hits {
        let Some(path) = hit.path.as_deref() else {
            continue;
        };
        if let Ok(Some(doc)) = store.find_active_by_source_path(path) {
            hit.buffer_id = doc.id;
        } else if let Ok(Some(doc)) = store.find_history_by_source_path(path) {
            hit.buffer_id = doc.id;
        }
    }

    Ok(SearchResults { hits, total })
}

/// Ranked note names for quick open.
///
/// Name-only, so the palette's list stays the notes themselves rather than the
/// lines inside them. Ranked by the same subsequence scorer the workspace file
/// palette uses, so a prefix of a name comes first.
#[tauri::command]
pub fn search_notes_by_name(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<FileHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    state
        .notes_index
        .search_names(&query, QUICK_OPEN_LIMIT)
        .map_err(|e| e.to_string())
}
