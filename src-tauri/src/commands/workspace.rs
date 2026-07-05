use std::path::Path;

use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use writ_core::workspace::WorkspaceEntry;
use writ_storage::workspace_store;

use crate::commands::config::persist_config;
use crate::state::AppState;
use crate::watcher::handler::start_workspace_watcher;

pub fn set_workspace_root_from_path(state: &AppState, raw: &Path) -> Result<String, String> {
    let canonical = crate::security::canonicalize_root(raw)
        .map_err(|e| format!("folder not accessible: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("not a directory: {}", canonical.display()));
    }

    {
        let mut config = state.config.lock().map_err(|e| e.to_string())?;
        config.workspace.root = Some(canonical.to_string_lossy().into_owned());
        persist_config(state, &config)?;
    }

    {
        let mut root = state.workspace_root.lock().map_err(|e| e.to_string())?;
        *root = Some(canonical.clone());
    }

    let handle = start_workspace_watcher(state.event_bus.clone(), canonical.clone())
        .map_err(|e| e.to_string())?;
    {
        let mut watcher = state.workspace_watcher.lock().map_err(|e| e.to_string())?;
        *watcher = Some(handle);
    }

    Ok(canonical.to_string_lossy().into_owned())
}

pub fn clear_workspace_root_inner(state: &AppState) -> Result<(), String> {
    {
        let mut config = state.config.lock().map_err(|e| e.to_string())?;
        config.workspace.root = None;
        persist_config(state, &config)?;
    }

    {
        let mut root = state.workspace_root.lock().map_err(|e| e.to_string())?;
        *root = None;
    }

    {
        let mut watcher = state.workspace_watcher.lock().map_err(|e| e.to_string())?;
        *watcher = None;
    }

    Ok(())
}

pub fn list_workspace_dir_inner(
    state: &AppState,
    dir_path: &str,
) -> Result<Vec<WorkspaceEntry>, String> {
    let root = state
        .workspace_root
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("no workspace folder is open")?;

    workspace_store::list_dir(&root, Path::new(dir_path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pick_workspace_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Option<tauri_plugin_dialog::FilePath>>();
    app.dialog()
        .file()
        .set_title("Open Folder")
        .pick_folder(move |path| {
            let _ = tx.send(path);
        });

    let picked = rx.recv().map_err(|e| e.to_string())?;
    let Some(fp) = picked else {
        return Ok(None);
    };
    let pb = fp.into_path().map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    let root = set_workspace_root_from_path(&state, &pb)?;
    Ok(Some(root))
}

#[tauri::command]
pub fn clear_workspace_root(state: State<'_, AppState>) -> Result<(), String> {
    clear_workspace_root_inner(&state)
}

#[tauri::command]
pub fn list_workspace_dir(
    state: State<'_, AppState>,
    dir_path: String,
) -> Result<Vec<WorkspaceEntry>, String> {
    list_workspace_dir_inner(&state, &dir_path)
}

#[tauri::command]
pub fn get_workspace_root(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let root = state.workspace_root.lock().map_err(|e| e.to_string())?;
    Ok(root.as_ref().map(|p| p.to_string_lossy().into_owned()))
}
