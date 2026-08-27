use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::poison::recover_poison;
use crate::security::{canonicalize_for_authorization, paths_equal_for_authorization};
use crate::state::AppState;
use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use writ_core::buffer::document::BufferDocument;
use writ_core::buffer::manager::BufferManager;
use writ_core::events::bus::WritEvent;
use writ_core::file_ops::{self, FileOpenMode};
use writ_core::watcher::change_event::ExternalChange;
use writ_storage::buffer_store::BufferStore;

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
    if state.is_within_inbox(&canonical) {
        return Ok(canonical);
    }
    if state.is_within_notes(&canonical) {
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

    open_file_classified(
        state,
        &canonical,
        classification.mode,
        classification.size_bytes,
    )
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
        resync_open_buffer(state, &store, &existing);
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
        store.restore(&history_buf.id).map_err(|e| e.to_string())?;
        // Reopening reads the file; it never writes it back. The index is
        // refreshed instead, which is the only thing the old write-back was
        // achieving now that the file is the only copy (ADR-028 §1).
        if let Err(e) = store.reindex_buffer(&history_buf.id) {
            tracing::debug!(buffer_id = %history_buf.id, error = %e, "reindex on reopening failed");
        }
        state
            .authorized_paths
            .record_blessed_source(canonical.to_string());
        let doc = store.get(&history_buf.id).map_err(|e| e.to_string())?;
        return Ok(FileOpenResult {
            mode,
            size_bytes,
            doc,
        });
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

    // No stamp: opening a file writes nothing, so there is no write of Writ's
    // own for a watcher to mistake for somebody else's.
    store
        .open_from_path(&new_doc, &content)
        .map_err(|e| e.to_string())?;

    state
        .authorized_paths
        .record_blessed_source(canonical.to_string());
    Ok(FileOpenResult {
        mode,
        size_bytes,
        doc: new_doc,
    })
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

/// Brings an already-open buffer back in line with its file on disk.
///
/// Reopening is how the user says "show me this file" — the CLI reopening a
/// list of paths, the OS handing Writ a document, a drop onto the window — so
/// disk wins over the copy Writ loaded earlier. The editor is told through the
/// same event an external edit raises, which reloads a clean buffer and asks
/// first when there are unsaved keystrokes to lose.
///
/// Best-effort: a file that cannot be read here still opens its tab.
///
/// Nothing is copied. The editor reloads through `read_buffer_content`, which
/// reads the file itself (ADR-028 §1), so the reload *is* the resync; the read
/// here is what the watcher stamp needs to recognise those bytes as ones Writ
/// already knows about.
fn resync_open_buffer(state: &AppState, store: &BufferStore, doc: &BufferDocument) {
    let Ok(source) = store.read_source(&doc.id) else {
        return;
    };
    {
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::file::resync_open_buffer",
        );
        let now = Instant::now();
        if let Some(path) = doc.source_path.as_deref() {
            ignore.record(path.to_string(), &source, now);
            if let Some(name) = Path::new(path).file_name() {
                ignore.record(name.to_string_lossy().into_owned(), &source, now);
            }
        }
    }
    if let Err(e) = store.reindex_buffer(&doc.id) {
        tracing::debug!(buffer_id = %doc.id, error = %e, "reindex after reopening failed");
    }
    state.event_bus.emit(WritEvent::BufferExternal {
        buffer_id: doc.id.clone(),
        change: ExternalChange::Modified,
    });
}

/// Gate on writing back to a buffer's originating file.
///
/// Two things have to hold. The path must still resolve to itself, so a file
/// swapped for a symlink after it was opened cannot redirect the write
/// somewhere else. And it must be blessed — recorded when the file was opened
/// through the dialog, a drop, the CLI, or the OS, and rehydrated at startup
/// from the persisted buffers — so a compromised webview cannot name an
/// arbitrary path and have Writ write it.
///
/// A path that no longer exists passes: a file deleted underneath an open
/// buffer is recreated by the next save, which is what the external-edit
/// policy promises. Nothing can be redirected in that case either, since
/// [`write_atomic`](writ_storage::atomic::write_atomic) renames onto the
/// literal path rather than following a dangling link.
pub fn authorize_source_write(state: &AppState, source_path: &str) -> Result<(), String> {
    if notes_containment_authorizes(state, source_path) {
        return Ok(());
    }
    if !state.authorized_paths.is_blessed_source(source_path) {
        return Err(ERR_UNAUTHORIZED_PATH.to_string());
    }
    match canonicalize_for_authorization(Path::new(source_path)) {
        Ok(canonical) if paths_equal_for_authorization(&canonical, source_path) => Ok(()),
        Ok(_) => Err(ERR_UNAUTHORIZED_PATH.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ERR_UNAUTHORIZED_PATH.to_string()),
    }
}

/// Containment half of the write gate: everything inside the notes folder is
/// writable without a per-file blessing.
///
/// A note that a sync client dropped into the folder was never opened through
/// a dialog, so nothing recorded it, and refusing its first save is the defect
/// ADR-028 §2 names.
///
/// Containment is decided on the path with every existing part resolved, so a
/// symlink cannot carry the write out of the folder — neither the file itself
/// nor a linked directory above it, which is the case a check on the leaf
/// alone misses. A path that does not exist yet is resolved as far as the
/// filesystem allows and the rest is appended, which is what lets a new note
/// be minted and a deleted note be recreated.
fn notes_containment_authorizes(state: &AppState, source_path: &str) -> bool {
    match resolve_for_containment(Path::new(source_path)) {
        Some(resolved) => state.is_within_notes(&resolved),
        None => false,
    }
}

/// Resolves `path` against the filesystem as far as it exists, then appends
/// the components that do not exist yet.
///
/// Walking up to the deepest existing ancestor is what makes the answer honest
/// for a file Writ is about to create: every symlink and every `..` above the
/// new name is resolved by `canonicalize`, and only names the filesystem has
/// never seen are appended literally.
///
/// Returns `None` for a relative path, for a path whose unresolved tail is
/// `..` or empty (`Path::file_name` yields nothing for either, so such a tail
/// can never be appended), and for any resolution error other than a missing
/// file.
fn resolve_for_containment(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }

    let mut unresolved: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path.to_path_buf();
    loop {
        match canonicalize_for_authorization(&cursor) {
            Ok(base) => {
                let mut resolved = PathBuf::from(base);
                for name in unresolved.iter().rev() {
                    resolved.push(name);
                }
                return resolved.into_os_string().into_string().ok();
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                unresolved.push(cursor.file_name()?.to_os_string());
                cursor = cursor.parent()?.to_path_buf();
            }
            Err(_) => return None,
        }
    }
}
