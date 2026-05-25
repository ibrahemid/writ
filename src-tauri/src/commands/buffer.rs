use std::time::Instant;

use crate::poison::recover_poison;
use crate::state::AppState;
use tauri::State;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::buffer::manager::BufferManager;
use writ_storage::buffer_store::BufferStore;

/// Outcome of resolving a new-buffer request: either an existing empty
/// scratch buffer to reuse, or a freshly minted (not yet persisted)
/// buffer to create.
pub enum CreateDecision {
    /// Reuse this already-persisted empty scratch buffer; no new row,
    /// no `updated_at` bump, no event is emitted.
    Reuse(BufferDocument),
    /// This buffer was just minted and must be persisted by the caller.
    Create(BufferDocument),
}

/// Decides whether a new-buffer request reuses an existing empty scratch
/// buffer or mints a new one.
///
/// An untitled request reuses the first active, never-renamed, zero-byte
/// scratch buffer if one exists, preventing empty buffers from piling up
/// when "new tab" is pressed repeatedly. An explicit title always mints.
/// Callers must flush pending autosave before calling so disk-read
/// emptiness reflects the live editor.
pub fn decide_create_buffer(
    store: &BufferStore,
    mgr: &mut BufferManager,
    title: Option<String>,
) -> Result<CreateDecision, String> {
    if title.is_none() {
        if let Some(existing) = store.find_empty_scratch_active().map_err(|e| e.to_string())? {
            return Ok(CreateDecision::Reuse(existing));
        }
    }
    let doc = mgr.create_buffer(title).map_err(|e| e.to_string())?;
    Ok(CreateDecision::Create(doc))
}

#[tauri::command]
pub fn create_buffer(
    state: State<'_, AppState>,
    title: Option<String>,
) -> Result<BufferDocument, String> {
    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let store = state.store.lock().map_err(|e| e.to_string())?;
    match decide_create_buffer(&store, &mut mgr, title)? {
        CreateDecision::Reuse(doc) => Ok(doc),
        CreateDecision::Create(doc) => {
            store.insert(&doc).map_err(|e| e.to_string())?;
            {
                let mut ignore = recover_poison(
                    state.watcher_ignore.lock(),
                    "commands::buffer::create_buffer",
                );
                ignore.record(doc.filename.clone(), b"", Instant::now());
            }
            store.save_content(&doc.id, "").map_err(|e| e.to_string())?;
            Ok(doc)
        }
    }
}

#[tauri::command]
pub fn get_buffer(state: State<'_, AppState>, id: String) -> Result<BufferDocument, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.get(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_buffer_content(
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(&id).map_err(|e| e.to_string())?;
    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::buffer::save_buffer_content",
        );
        ignore.record(doc.filename.clone(), content.as_bytes(), Instant::now());
    }
    store.save_content(&id, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_buffer_content(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.read_content(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_active_buffers(state: State<'_, AppState>) -> Result<Vec<BufferDocument>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .list_by_status(BufferStatus::Active)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(&id).map_err(|e| e.to_string())?;
    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::buffer::close_buffer",
        );
        ignore.remove(&doc.filename);
    }
    store.close(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_buffers(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.close_many(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(&id).ok();
    if let Some(doc) = doc.as_ref() {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::buffer::delete_buffer",
        );
        ignore.remove(&doc.filename);
    }
    store.delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_tab_order(state: State<'_, AppState>, id: String, order: u32) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .update_tab_order(&id, order)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_buffer(state: State<'_, AppState>, id: String, title: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.rename(&id, &title).map_err(|e| e.to_string())
}
