use std::path::Path;
use std::time::Instant;

use crate::poison::recover_poison;
use crate::state::AppState;
use tauri::State;
use writ_core::buffer::document::BufferDocument;
use writ_core::buffer::manager::BufferManager;
use writ_core::file_ops;

pub fn open_file_from_path(state: &AppState, path: &str) -> Result<BufferDocument, String> {
    let file_path = Path::new(path);

    file_ops::validate_file_for_opening(file_path).map_err(|e| e.to_string())?;

    let store = state.store.lock().map_err(|e| e.to_string())?;

    if let Some(existing) = store
        .find_active_by_source_path(path)
        .map_err(|e| e.to_string())?
    {
        return Ok(existing);
    }

    if let Some(history_buf) = store
        .find_history_by_source_path(path)
        .map_err(|e| e.to_string())?
    {
        store.restore(&history_buf.id).map_err(|e| e.to_string())?;
        let content = std::fs::read_to_string(file_path).map_err(|e| e.to_string())?;
        {
            let mut ignore = recover_poison(
                state.watcher_ignore.lock(),
                "commands::file::open_file_from_path:history",
            );
            ignore.record(history_buf.filename.clone(), content.as_bytes(), Instant::now());
        }
        store
            .save_content(&history_buf.id, &content)
            .map_err(|e| e.to_string())?;
        return store.get(&history_buf.id).map_err(|e| e.to_string());
    }

    let content = std::fs::read_to_string(file_path).map_err(|e| e.to_string())?;

    let language = file_ops::detect_language_from_path(file_path);

    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let doc = mgr
        .open_external(path.to_string())
        .map_err(|e| e.to_string())?;

    let doc = BufferDocument { language, ..doc };

    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::file::open_file_from_path:new",
        );
        ignore.record(doc.filename.clone(), content.as_bytes(), Instant::now());
    }

    store
        .open_from_path(&doc, &content)
        .map_err(|e| e.to_string())?;

    Ok(doc)
}

#[tauri::command]
pub fn open_file(state: State<'_, AppState>, path: String) -> Result<BufferDocument, String> {
    open_file_from_path(&state, &path)
}

#[tauri::command]
pub fn save_to_source(
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;

    {
        let doc = store.get(&id).map_err(|e| e.to_string())?;
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::file::save_to_source",
        );
        ignore.record(doc.filename.clone(), content.as_bytes(), Instant::now());
    }

    store
        .save_to_source(&id, &content)
        .map_err(|e| e.to_string())
}
