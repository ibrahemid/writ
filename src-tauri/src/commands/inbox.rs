use std::path::Path;

use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use writ_core::inbox::InboxFile;
use writ_storage::inbox_store;

use crate::commands::config::persist_config;
use crate::commands::offload::{run_blocking, ERR_COMMAND_STOPPED};
use crate::state::AppState;
use crate::watcher::handler::start_inbox_watcher;

pub fn set_inbox_path_from_path(state: &AppState, raw: &Path) -> Result<String, String> {
    let canonical = crate::security::canonicalize_root(raw)
        .map_err(|e| format!("folder not accessible: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("not a directory: {}", canonical.display()));
    }

    {
        let mut config = state.config.lock().map_err(|e| e.to_string())?;
        config.inbox.path = Some(canonical.to_string_lossy().into_owned());
        persist_config(state, &config)?;
    }

    {
        let mut root = state.inbox_root.lock().map_err(|e| e.to_string())?;
        *root = Some(canonical.clone());
    }

    let handle = start_inbox_watcher(
        state.event_bus.clone(),
        canonical.clone(),
        state.watcher_ignore.clone(),
    )
    .map_err(|e| e.to_string())?;
    {
        let mut watcher = state.inbox_watcher.lock().map_err(|e| e.to_string())?;
        *watcher = Some(handle);
    }

    Ok(canonical.to_string_lossy().into_owned())
}

pub fn clear_inbox_inner(state: &AppState) -> Result<(), String> {
    {
        let mut config = state.config.lock().map_err(|e| e.to_string())?;
        config.inbox.path = None;
        persist_config(state, &config)?;
    }

    {
        let mut root = state.inbox_root.lock().map_err(|e| e.to_string())?;
        *root = None;
    }

    {
        let mut watcher = state.inbox_watcher.lock().map_err(|e| e.to_string())?;
        *watcher = None;
    }

    Ok(())
}

#[tauri::command]
pub async fn pick_inbox_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<tauri_plugin_dialog::FilePath>>();
    app.dialog()
        .file()
        .set_title("Watch Folder")
        .pick_folder(move |path| {
            let _ = tx.send(path);
        });

    let picked = rx.await.map_err(|e| e.to_string())?;
    let Some(fp) = picked else {
        return Ok(None);
    };
    let pb = fp.into_path().map_err(|e| e.to_string())?;
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        set_inbox_path_from_path(state, &pb).map(Some)
    })
    .await
}

#[tauri::command]
pub fn clear_inbox(state: State<'_, AppState>) -> Result<(), String> {
    clear_inbox_inner(&state)
}

#[tauri::command]
pub fn get_inbox_path(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let root = state.inbox_root.lock().map_err(|e| e.to_string())?;
    Ok(root.as_ref().map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn list_inbox_files<R: Runtime>(app: AppHandle<R>) -> Result<Vec<InboxFile>, String> {
    run_blocking(app, ERR_COMMAND_STOPPED, |state| {
        let root = state.inbox_root.lock().map_err(|e| e.to_string())?.clone();
        match root {
            Some(root) => inbox_store::list_files(&root).map_err(|e| e.to_string()),
            None => Ok(Vec::new()),
        }
    })
    .await
}
