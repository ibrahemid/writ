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

    // The digest is carried alongside the content rather than recomputed from
    // it: for the binary tier `content` is a hex dump, not the file's bytes,
    // and for every tier re-reading a large file just to hash it would defeat
    // the point of the large-file tiers existing at all.
    let (content, digest) = if is_binary {
        let bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let digest = writ_core::hash::sha256_bytes(&bytes);
        (
            file_ops::generate_hex_dump(&bytes, size_bytes as usize),
            digest,
        )
    } else {
        let text = std::fs::read_to_string(file_path).map_err(|e| e.to_string())?;
        let digest = writ_core::hash::sha256_bytes(text.as_bytes());
        (text, digest)
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
        state.record_disk_state(&history_buf.id, file_path, digest, size_bytes);
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
    state.record_disk_state(&new_doc.id, file_path, digest, size_bytes);
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
/// first when there are unsaved keystrokes to lose. The event only fires when
/// the file's digest actually moved since Writ last read or wrote it: a
/// reopen of a file nothing touched is not an external change, and emitting
/// for it anyway would reload (or prompt over) an editor that has nothing new
/// to show.
///
/// Best-effort: a file that cannot be read here still opens its tab.
///
/// Nothing is copied. The editor reloads through `read_buffer_content`, which
/// reads the file itself (ADR-028 §1), so the reload *is* the resync; the read
/// here is what the watcher stamp needs to recognise those bytes as ones Writ
/// already knows about.
///
/// What the file held is deliberately not recorded here. The event is an offer
/// the editor can decline — it asks first when there are unsaved keystrokes to
/// lose — and recording on the strength of having emitted would leave a user
/// who said no with a tab Writ believes is current, and the next reopen
/// silent. The record moves when the file is actually read, which the reload
/// does through [`crate::commands::buffer::read_buffer_content`].
///
/// The reindex is skipped for a read-only buffer. A binary one would not
/// usefully index anyway (the file's bytes are not valid text), and a
/// generated document must never index its body no matter how often it is
/// reopened (ADR-028 §1) — [`open_generated_document`] seeds its title-only
/// entry once and nothing here may overwrite that with the full text.
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
            let key = writ_core::watcher::ignore::source_key(
                &crate::watcher::handler::ignore_key_path(Path::new(path)),
            );
            ignore.record(key, &source, now);
        }
    }
    if !doc.read_only {
        if let Err(e) = store.reindex_buffer(&doc.id) {
            tracing::debug!(buffer_id = %doc.id, error = %e, "reindex after reopening failed");
        }
    }
    if !state.disk_hash_matches(&doc.id, &source) {
        state.event_bus.emit(WritEvent::BufferExternal {
            buffer_id: doc.id.clone(),
            change: ExternalChange::Modified,
        });
    }
}

/// Writes `content` to the fixed path a generated document titled `title`
/// takes under the data directory, and opens it as a source-backed,
/// read-only buffer.
///
/// A document Writ writes rather than the user must never mint a file in the
/// notes folder (ADR-028 §1): unlike a plain note, its path is decided here,
/// not by [`crate::notes::attach_note_file`], and `content` is written before
/// the buffer exists rather than on a later first keystroke. The row is
/// read-only, so
/// [`write_source_guarded`](writ_storage::buffer_store::BufferStore::write_source_guarded)'s
/// existing refusal is what stops a save of it — nothing new is checked for
/// that.
///
/// The row is found the same way [`open_file_classified`] finds one, by the
/// canonical path: a tab still open is resynced in place, and a closed one is
/// restored. Without the second lookup, closing the tab and opening the
/// document again would mint a row per open and fill History with a document
/// the user never wrote. `content` overwrites whatever the file held before,
/// so a second call regenerates the same file rather than minting a dedupe
/// sibling.
pub fn open_generated_document(
    state: &AppState,
    title: &str,
    content: &str,
) -> Result<FileOpenResult, String> {
    let path = crate::generated::generated_document_path(&state.writ_dir, title);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    let canonical = canonicalize_for_authorization(&path).map_err(|e| e.to_string())?;
    let size_bytes = content.len() as u64;

    let store = state.store.lock().map_err(|e| e.to_string())?;

    if let Some(existing) = store
        .find_active_by_source_path(&canonical)
        .map_err(|e| e.to_string())?
    {
        state
            .authorized_paths
            .record_blessed_source(canonical.clone());
        resync_open_buffer(state, &store, &existing);
        return Ok(FileOpenResult {
            mode: FileOpenMode::Normal,
            size_bytes,
            doc: existing,
        });
    }

    if let Some(closed) = store
        .find_history_by_source_path(&canonical)
        .map_err(|e| e.to_string())?
    {
        store.restore(&closed.id).map_err(|e| e.to_string())?;
        state
            .authorized_paths
            .record_blessed_source(canonical.clone());
        state.record_disk_state_bytes(&closed.id, &path, content.as_bytes());
        let doc = store.get(&closed.id).map_err(|e| e.to_string())?;
        return Ok(FileOpenResult {
            mode: FileOpenMode::Normal,
            size_bytes,
            doc,
        });
    }

    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let new_doc = mgr
        .open_external(canonical.clone())
        .map_err(|e| e.to_string())?;
    let new_doc = BufferDocument {
        title: title.to_string(),
        read_only: true,
        size_bytes,
        ..new_doc
    };
    store
        .open_from_path_unindexed(&new_doc)
        .map_err(|e| e.to_string())?;
    state.authorized_paths.record_blessed_source(canonical);
    state.record_disk_state_bytes(&new_doc.id, &path, content.as_bytes());
    Ok(FileOpenResult {
        mode: FileOpenMode::Normal,
        size_bytes,
        doc: new_doc,
    })
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
/// The watcher's ignore keys are built from this too
/// ([`crate::watcher::handler::ignore_key_path`]), so a file being created is
/// stamped under the path the watcher will deliver for it.
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
pub(crate) fn resolve_for_containment(path: &Path) -> Option<String> {
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
