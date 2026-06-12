use std::path::Path;
use std::time::Instant;

use crate::poison::recover_poison;
use crate::security::canonicalize_for_authorization;
use crate::state::AppState;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use writ_core::buffer::document::BufferDocument;
use writ_core::buffer::manager::BufferManager;
use writ_core::file_ops;

const ERR_UNAUTHORIZED_PATH: &str =
    "path not authorized: open files via the dialog or by dropping them onto the window";

fn authorize_open(state: &AppState, raw_path: &str) -> Result<String, String> {
    let canonical = canonicalize_for_authorization(Path::new(raw_path))
        .map_err(|_| ERR_UNAUTHORIZED_PATH.to_string())?;
    if state.authorized_paths.consume_for_open(&canonical) {
        return Ok(canonical);
    }
    if state.is_within_workspace(&canonical) {
        return Ok(canonical);
    }
    Err(ERR_UNAUTHORIZED_PATH.to_string())
}

pub fn open_file_from_path(state: &AppState, path: &str) -> Result<BufferDocument, String> {
    let canonical = authorize_open(state, path)?;
    let file_path = Path::new(&canonical);

    file_ops::validate_file_for_opening(file_path).map_err(|e| e.to_string())?;

    let store = state.store.lock().map_err(|e| e.to_string())?;

    if let Some(existing) = store
        .find_active_by_source_path(&canonical)
        .map_err(|e| e.to_string())?
    {
        state.authorized_paths.record_blessed_source(canonical);
        return Ok(existing);
    }

    if let Some(history_buf) = store
        .find_history_by_source_path(&canonical)
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
        state.authorized_paths.record_blessed_source(canonical);
        return store.get(&history_buf.id).map_err(|e| e.to_string());
    }

    let content = std::fs::read_to_string(file_path).map_err(|e| e.to_string())?;

    let language = file_ops::detect_language_from_path(file_path);

    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let doc = mgr
        .open_external(canonical.clone())
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

    state.authorized_paths.record_blessed_source(canonical);
    Ok(doc)
}

#[tauri::command]
pub fn open_file(state: State<'_, AppState>, path: String) -> Result<BufferDocument, String> {
    open_file_from_path(&state, &path)
}

#[tauri::command]
pub async fn pick_files_to_open(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Option<Vec<tauri_plugin_dialog::FilePath>>>();
    app.dialog()
        .file()
        .set_title("Open File")
        .pick_files(move |paths| {
            let _ = tx.send(paths);
        });

    let paths = rx.recv().map_err(|e| e.to_string())?;
    let Some(paths) = paths else {
        return Ok(Vec::new());
    };

    let state = app.state::<AppState>();
    let mut out = Vec::with_capacity(paths.len());
    for fp in paths {
        let pb = match fp.into_path() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "dialog returned non-path entry; skipping");
                continue;
            }
        };
        match canonicalize_for_authorization(&pb) {
            Ok(canonical) => {
                state.authorized_paths.record_for_open(canonical.clone());
                out.push(canonical);
            }
            Err(e) => {
                tracing::warn!(error = %e, path = %pb.display(), "failed to canonicalize dialog path; skipping");
            }
        }
    }
    Ok(out)
}

pub fn save_to_source_for_test(
    state: &AppState,
    id: String,
    content: String,
) -> Result<(), String> {
    save_to_source_inner(state, id, content)
}

fn save_to_source_inner(state: &AppState, id: String, content: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(&id).map_err(|e| e.to_string())?;
    let source_path = doc
        .source_path
        .as_deref()
        .ok_or_else(|| "buffer has no source_path".to_string())?;

    let canonical = canonicalize_for_authorization(Path::new(source_path))
        .map_err(|_| ERR_UNAUTHORIZED_PATH.to_string())?;

    if canonical != source_path {
        return Err(ERR_UNAUTHORIZED_PATH.to_string());
    }
    if !state.authorized_paths.is_blessed_source(&canonical) {
        return Err(ERR_UNAUTHORIZED_PATH.to_string());
    }

    {
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

#[tauri::command]
pub fn save_to_source(
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<(), String> {
    save_to_source_inner(&state, id, content)
}
