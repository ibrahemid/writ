use crate::poison::recover_poison;
use crate::state::AppState;
use tauri::State;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::buffer::manager::BufferManager;

#[tauri::command]
pub fn create_buffer(
    state: State<'_, AppState>,
    title: Option<String>,
) -> Result<BufferDocument, String> {
    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let doc = mgr.create_buffer(title).map_err(|e| e.to_string())?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.insert(&doc).map_err(|e| e.to_string())?;
    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::buffer::create_buffer",
        );
        ignore.insert(doc.filename.clone());
    }
    store.save_content(&doc.id, "").map_err(|e| e.to_string())?;
    Ok(doc)
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
        ignore.insert(doc.filename.clone());
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
    store.close(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
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
