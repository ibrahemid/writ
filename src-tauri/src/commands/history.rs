use crate::commands::file::FileOpenResult;
use crate::state::AppState;
use serde::Serialize;
use tauri::State;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::file_ops::{self, FileOpenMode};
use writ_core::search::SearchHit;
use writ_core::workspace::file_search::FileHit;
use writ_storage::buffer_store::BufferStore;

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

/// IPC: [`restore_buffer_inner`].
#[tauri::command]
pub fn restore_buffer(state: State<'_, AppState>, id: String) -> Result<FileOpenResult, String> {
    restore_buffer_inner(&state, &id)
}

/// IPC: [`restore_buffer_confirmed_inner`].
#[tauri::command]
pub fn restore_buffer_confirmed(
    state: State<'_, AppState>,
    id: String,
) -> Result<FileOpenResult, String> {
    restore_buffer_confirmed_inner(&state, &id)
}

/// Brings a closed file back as an open tab, answering as an open does.
///
/// Reopening reads the file, and it may have grown while it was closed: one
/// now over the size Writ opens without asking answers
/// [`FileOpenMode::LargeFileConfirm`] and stays closed until
/// [`restore_buffer_confirmed_inner`], and one over the size Writ opens is
/// refused. The size comes from a stat, never a read, since reading a file
/// whose bytes are with a sync provider fetches them. A file that is not there
/// reopens as it always did, and its tab says what became of it.
///
/// A tab restored at launch whose file was over the size Writ opens without
/// asking answers the same way when it is shown. It was never closed, so it
/// stays where it is and nothing is restored.
///
/// The tab starts being followed again here. Its watch went when it closed,
/// and the file has had every moment since then to change. A tab restored at
/// launch has been followed since the launch listed it, and is not followed
/// again: on a volume with no file id, naming the file reads all of it.
///
/// A closed file another tab has open answers with that tab, and the closed
/// row stays closed: a file is one open tab, and two tabs on it would each
/// save over the other. Save As can give a note a file that is still in the
/// history list under a row of its own.
pub fn restore_buffer_inner(state: &AppState, id: &str) -> Result<FileOpenResult, String> {
    reopen_closed(state, id, false)
}

/// [`restore_buffer_inner`] once the person said Open. The size is asked
/// again, since the file can grow while the question is up.
pub fn restore_buffer_confirmed_inner(
    state: &AppState,
    id: &str,
) -> Result<FileOpenResult, String> {
    reopen_closed(state, id, true)
}

fn reopen_closed(state: &AppState, id: &str, confirmed: bool) -> Result<FileOpenResult, String> {
    let (doc, mode, size_bytes, reopened) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let closed = store.get(id).map_err(|e| e.to_string())?;
        if let Some(open) = open_elsewhere(&store, &closed)? {
            let mode = file_ops::classify_file(open.size_bytes, open.read_only).once_open();
            return Ok(FileOpenResult {
                mode,
                size_bytes: open.size_bytes,
                doc: Some(open),
            });
        }
        let size_now = crate::commands::buffer::measured_size(&closed);
        let (mode, size_bytes) = match size_now {
            Some(size) => (file_ops::classify_size(size, closed.read_only), size),
            None => (
                file_ops::classify_file(closed.size_bytes, closed.read_only).once_open(),
                closed.size_bytes,
            ),
        };
        let asks = mode == FileOpenMode::LargeFileConfirm && !confirmed;
        if asks || matches!(mode, FileOpenMode::Refused { .. }) {
            return Ok(FileOpenResult {
                mode,
                size_bytes,
                doc: None,
            });
        }
        let reopened = closed.status == BufferStatus::History;
        if reopened {
            store.restore(id).map_err(|e| e.to_string())?;
        }
        // The editor reads the tier off the row, so the row carries the size
        // the file has now rather than the size it had when it was closed.
        if size_bytes != closed.size_bytes {
            store
                .record_size(id, size_bytes)
                .map_err(|e| e.to_string())?;
        }
        let doc = store.get(id).map_err(|e| e.to_string())?;
        (doc, mode.once_open(), size_bytes, reopened)
    };
    if reopened {
        state.follow_note_file(&doc);
    }
    Ok(FileOpenResult {
        mode,
        size_bytes,
        doc: Some(doc),
    })
}

/// The open tab on the file a closed row names, when one other than the row
/// itself has it open.
fn open_elsewhere(
    store: &BufferStore,
    closed: &BufferDocument,
) -> Result<Option<BufferDocument>, String> {
    if closed.status != BufferStatus::History {
        return Ok(None);
    }
    let Some(path) = closed.source_path.as_deref() else {
        return Ok(None);
    };
    let open = store
        .find_active_by_source_path(path)
        .map_err(|e| e.to_string())?;
    Ok(open.filter(|open| open.id != closed.id))
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
        .search_names(&query, &state.notes_root(), QUICK_OPEN_LIMIT)
        .map_err(|e| e.to_string())
}
