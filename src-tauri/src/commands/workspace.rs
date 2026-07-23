use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use writ_core::workspace::file_search::FileHit;
use writ_core::workspace::WorkspaceEntry;
use writ_storage::workspace_grep::{self, ContentHit, GrepLimits, GrepOutcome, GrepRequest};
use writ_storage::workspace_store;

use crate::commands::config::persist_config;
use crate::state::AppState;
use crate::watcher::handler::start_workspace_watcher;
use crate::workspace_index::{self, IndexStatus};

/// Upper bound on file-name hits returned to the palette.
const FILE_SEARCH_LIMIT: usize = 100;

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

    workspace_index::set_root_and_rebuild(&state.workspace_index, Some(canonical.clone()));

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

    workspace_index::set_root_and_rebuild(&state.workspace_index, None);

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

pub fn search_workspace_files_inner(state: &AppState, query: &str) -> Vec<FileHit> {
    state
        .workspace_index
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .search(query, FILE_SEARCH_LIMIT)
}

pub fn workspace_index_status_inner(state: &AppState) -> IndexStatus {
    state
        .workspace_index
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .status()
}

/// Ranks workspace file names against `query` via the in-memory index.
#[tauri::command]
pub fn search_workspace_files(state: State<'_, AppState>, query: String) -> Vec<FileHit> {
    search_workspace_files_inner(&state, &query)
}

/// Reports the file-name index size and whether it is truncated.
#[tauri::command]
pub fn workspace_index_status(state: State<'_, AppState>) -> IndexStatus {
    workspace_index_status_inner(&state)
}

/// One streamed batch of content-search results. The final batch carries the
/// [`GrepOutcome`]; every batch is stamped with its `generation` so the UI can
/// discard batches from a superseded query.
#[derive(Debug, Clone, Serialize)]
pub struct SearchBatch {
    /// Generation this batch belongs to.
    pub generation: u64,
    /// Hits in this batch (empty on the terminal outcome batch).
    pub hits: Vec<ContentHit>,
    /// Present only on the final batch.
    pub outcome: Option<GrepOutcome>,
}

/// Runs one content search, streaming every batch (and the terminal
/// outcome batch) through `emit`. Bumps `counter` and captures its value so a
/// newer search cancels this one; each batch is stamped with that generation.
/// Synchronous and free of Tauri types, so it is unit-testable; the command
/// wraps it on a blocking thread and forwards `emit` to the IPC channel.
pub fn run_content_search(
    root: std::path::PathBuf,
    counter: Arc<std::sync::atomic::AtomicU64>,
    query: String,
    limits: GrepLimits,
    emit: Arc<dyn Fn(SearchBatch) + Send + Sync>,
) -> Result<GrepOutcome, String> {
    let generation = counter.fetch_add(1, Ordering::SeqCst) + 1;

    let cancelled: Arc<dyn Fn() -> bool + Send + Sync> = {
        let counter = counter.clone();
        Arc::new(move || counter.load(Ordering::SeqCst) != generation)
    };

    let on_hits: Box<dyn FnMut(Vec<ContentHit>) + Send> = {
        let emit = emit.clone();
        Box::new(move |hits| {
            emit(SearchBatch {
                generation,
                hits,
                outcome: None,
            });
        })
    };

    let outcome = workspace_grep::search_workspace_content(
        GrepRequest {
            root,
            query,
            generation,
        },
        limits,
        cancelled,
        on_hits,
    )
    .map_err(|e| e.to_string())?;

    emit(SearchBatch {
        generation,
        hits: Vec::new(),
        outcome: Some(outcome),
    });

    Ok(outcome)
}

/// Streams content-search hits over `on_batch`. Bumps the search generation so
/// an in-flight older search is cancelled at its next check; the final batch
/// carries the [`GrepOutcome`] with the caps the UI must surface (ADR-026).
#[tauri::command]
pub async fn search_workspace_content(
    state: State<'_, AppState>,
    query: String,
    on_batch: Channel<SearchBatch>,
) -> Result<(), String> {
    let root = state
        .workspace_root
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("no workspace folder is open")?;
    let counter = state.search_generation.clone();

    let emit: Arc<dyn Fn(SearchBatch) + Send + Sync> = Arc::new(move |batch| {
        let _ = on_batch.send(batch);
    });

    tauri::async_runtime::spawn_blocking(move || {
        run_content_search(root, counter, query, GrepLimits::default(), emit)
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|_| ())
}
