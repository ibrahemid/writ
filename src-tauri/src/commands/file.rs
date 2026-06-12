use std::path::Path;
use std::time::Instant;

use crate::poison::recover_poison;
use crate::security::canonicalize_for_authorization;
use crate::state::AppState;
use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use writ_core::buffer::document::BufferDocument;
use writ_core::buffer::manager::BufferManager;
use writ_core::file_ops::{self, FileOpenMode};

const ERR_UNAUTHORIZED_PATH: &str =
    "path not authorized: open files via the dialog or by dropping them onto the window";

/// Returned to the frontend for every `open_file` call.
///
/// Carries the buffer metadata plus the mode tier so the frontend can
/// configure the editor without a second IPC round-trip.
#[derive(Debug, Clone, Serialize)]
pub struct FileOpenResult {
    /// The buffer metadata row.
    pub doc: BufferDocument,
    /// How the file was classified.
    pub mode: FileOpenMode,
    /// File size in bytes (mirrors `doc.size_bytes`; included for
    /// convenience so callers do not have to traverse the nested struct).
    pub size_bytes: u64,
}

/// Returned when a file requires confirmation before loading.
///
/// The frontend shows a dialog, then calls `open_file_confirmed`.
#[derive(Debug, Clone, Serialize)]
pub struct FileOpenConfirmRequired {
    /// Canonical path that was classified.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Human-readable description of what will be disabled.
    pub warning: String,
}

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

/// Opens a file from an already-authorized canonical path.
///
/// Does not read the file's full content for the `LargeFileConfirm` tier —
/// returns early with an error containing the confirmation sentinel instead.
/// The frontend must call `open_file_confirmed` after the user confirms.
pub fn open_file_from_path(state: &AppState, path: &str) -> Result<FileOpenResult, String> {
    let canonical = authorize_open(state, path)?;
    let file_path = Path::new(&canonical);

    let classification = file_ops::classify_path(file_path).map_err(|e| e.to_string())?;

    match &classification.mode {
        FileOpenMode::Refused { reason } => return Err(reason.clone()),
        FileOpenMode::LargeFileConfirm => {
            return Err(format!(
                "__CONFIRM_REQUIRED__:{}:{}",
                canonical, classification.size_bytes
            ));
        }
        _ => {}
    }

    open_file_classified(state, &canonical, classification.mode, classification.size_bytes)
}

/// Performs the actual open after the frontend has confirmed.
///
/// Called for the 50–500 MiB tier after `open_file` returns the confirmation
/// sentinel and the user presses "Open anyway". The path must already be
/// authorized (the original `open_file` call consumed the authorization token
/// before returning the sentinel). Re-authorization is performed here via the
/// workspace membership check or a freshly recorded token.
fn open_file_classified(
    state: &AppState,
    canonical: &str,
    mode: FileOpenMode,
    size_bytes: u64,
) -> Result<FileOpenResult, String> {
    let file_path = Path::new(canonical);
    let store = state.store.lock().map_err(|e| e.to_string())?;

    if let Some(existing) = store
        .find_active_by_source_path(canonical)
        .map_err(|e| e.to_string())?
    {
        state
            .authorized_paths
            .record_blessed_source(canonical.to_string());
        let existing_mode = file_ops::classify_file(existing.size_bytes, existing.read_only);
        return Ok(FileOpenResult {
            mode: existing_mode,
            size_bytes: existing.size_bytes,
            doc: existing,
        });
    }

    let is_binary = matches!(mode, FileOpenMode::Binary);

    let content = if is_binary {
        let bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        file_ops::generate_hex_dump(&bytes, size_bytes as usize)
    } else {
        std::fs::read_to_string(file_path).map_err(|e| e.to_string())?
    };

    if let Some(history_buf) = store
        .find_history_by_source_path(canonical)
        .map_err(|e| e.to_string())?
    {
        store
            .restore(&history_buf.id)
            .map_err(|e| e.to_string())?;
        {
            let mut ignore = recover_poison(
                state.watcher_ignore.lock(),
                "commands::file::open_file_from_path:history",
            );
            ignore.record(
                history_buf.filename.clone(),
                content.as_bytes(),
                Instant::now(),
            );
        }
        store
            .save_content(&history_buf.id, &content)
            .map_err(|e| e.to_string())?;
        state
            .authorized_paths
            .record_blessed_source(canonical.to_string());
        let doc = store
            .get(&history_buf.id)
            .map_err(|e| e.to_string())?;
        return Ok(FileOpenResult { mode, size_bytes, doc });
    }

    let language = file_ops::detect_language_from_path(file_path);

    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let new_doc = mgr
        .open_external(canonical.to_string())
        .map_err(|e| e.to_string())?;

    let new_doc = BufferDocument {
        language,
        read_only: is_binary,
        size_bytes,
        ..new_doc
    };

    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::file::open_file_from_path:new",
        );
        ignore.record(new_doc.filename.clone(), content.as_bytes(), Instant::now());
    }

    store
        .open_from_path(&new_doc, &content)
        .map_err(|e| e.to_string())?;

    state
        .authorized_paths
        .record_blessed_source(canonical.to_string());
    Ok(FileOpenResult { mode, size_bytes, doc: new_doc })
}

#[tauri::command]
pub fn open_file(state: State<'_, AppState>, path: String) -> Result<FileOpenResult, String> {
    open_file_from_path(&state, &path)
}

/// Opens a file in the 50–500 MiB tier after explicit user confirmation.
///
/// The caller must ensure the path was previously classified as
/// `LargeFileConfirm` — this command skips the tier check and opens
/// unconditionally at large-file mode.
#[tauri::command]
pub fn open_file_confirmed(
    state: State<'_, AppState>,
    path: String,
) -> Result<FileOpenResult, String> {
    let canonical = authorize_open(&state, &path)?;
    let file_path = Path::new(&canonical);
    let classification = file_ops::classify_path(file_path).map_err(|e| e.to_string())?;
    if let FileOpenMode::Refused { reason } = &classification.mode {
        return Err(reason.clone());
    }
    // Treat LargeFileConfirm as LargeFile now that the user confirmed.
    let mode = if classification.mode == FileOpenMode::LargeFileConfirm {
        FileOpenMode::LargeFile
    } else {
        classification.mode
    };
    open_file_classified(&state, &canonical, mode, classification.size_bytes)
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

    if doc.read_only {
        return Err(format!("buffer {} is read-only", id));
    }

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
