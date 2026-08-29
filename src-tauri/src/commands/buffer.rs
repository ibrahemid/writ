use std::path::Path;
use std::time::Instant;

use crate::fts_scheduler::{PollOutcome, FTS_REINDEX_DEBOUNCE};
use crate::poison::recover_poison;
use crate::state::AppState;
use tauri::{AppHandle, Manager, State};
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::buffer::manager::BufferManager;
use writ_storage::buffer_store::BufferStore;
use writ_storage::errors::StorageError;

/// Code a save carries when the file changed under Writ and the write was
/// stopped.
///
/// The frontend reads the code and writes its own sentence, so no wording a
/// person sees is load-bearing across the boundary and the message after the
/// code stays free to say whatever a log needs
/// (`ERR_UNAUTHORIZED_PATH` in [`crate::commands::file`] is the same shape).
pub const ERR_FILE_CHANGED_ON_DISK: &str = "ERR_FILE_CHANGED_ON_DISK";

/// Code a save carries when the file's bytes are not on this machine yet.
pub const ERR_FILE_NOT_DOWNLOADED: &str = "ERR_FILE_NOT_DOWNLOADED";

/// Renders a failed save for the frontend: a stable code first when the
/// failure is one the editor has something to say about, the plain message
/// otherwise.
pub fn save_failure_message(error: &StorageError) -> String {
    match error {
        StorageError::SourceChangedOnDisk { .. } => {
            format!("{ERR_FILE_CHANGED_ON_DISK}: {error}")
        }
        StorageError::SourceNotDownloaded { .. } => {
            format!("{ERR_FILE_NOT_DOWNLOADED}: {error}")
        }
        other => other.to_string(),
    }
}

/// Outcome of resolving a new-note request: either an existing note that has
/// not reached a file to reuse, or a freshly minted (not yet persisted) one.
pub enum CreateDecision {
    /// Reuse this already-persisted note; no new row, no `updated_at` bump,
    /// no event is emitted.
    Reuse(BufferDocument),
    /// This note was just minted and must be persisted by the caller.
    Create(BufferDocument),
}

/// Decides whether a new-note request reuses an existing note that never
/// reached a file, or mints a new one.
///
/// An untitled request reuses the first active note with no file if one
/// exists, so pressing "new tab" repeatedly does not pile rows up. An explicit
/// title always mints. Callers must flush any pending autosave first: that
/// save is what attaches the file, so an unflushed keystroke leaves the row
/// looking reusable.
pub fn decide_create_buffer(
    store: &BufferStore,
    mgr: &mut BufferManager,
    title: Option<String>,
) -> Result<CreateDecision, String> {
    if title.is_none() {
        if let Some(existing) = store
            .find_empty_scratch_active()
            .map_err(|e| e.to_string())?
        {
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
            // Nothing is written to disk here. A new note reaches a file on
            // its first keystroke and not before (ADR-028 §2), so pressing
            // Cmd+T and changing your mind leaves the notes folder as it was.
            store.insert(&doc).map_err(|e| e.to_string())?;
            Ok(doc)
        }
    }
}

#[tauri::command]
pub fn get_buffer(state: State<'_, AppState>, id: String) -> Result<BufferDocument, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.get(&id).map_err(|e| e.to_string())
}

/// Writes `content` for note `id` into the file it lives in, giving it one
/// first when it has none.
///
/// The destination is decided here rather than by the caller: autosave, the
/// flush on closing a tab, and the flush on quitting all arrive through the
/// one IPC command, and every note has to reach its file through every one of
/// them. The frontend never names a path — the row in the database does.
///
/// A note with nothing in it and no file yet writes nothing: a new tab the
/// user opened and has not typed into is not a note, and minting a file for
/// it would fill the folder with blank days.
///
/// Writes and stamps immediately; the FTS reindex is deferred off the
/// keystroke loop (ADR-020). The bytes on disk are durable on return; only
/// search freshness lags, bounded by the idle window and the shutdown flush.
///
/// A write that would land over a change Writ never read is stopped and the
/// text it was carrying is written beside the note instead
/// ([`writ_storage::buffer_store::BufferStore::write_source_guarded`]). What
/// it compares against is this process's record for the tab, so a note whose
/// tab was closed and reopened is measured against its file rather than
/// against a record of what the file used to hold. Such a failure comes back
/// under a stable code ([`save_failure_message`]).
pub fn save_buffer_content_inner(state: &AppState, id: &str, content: &str) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    if doc.read_only {
        return Err(format!("note {id} is read-only"));
    }

    let source_path = match doc.source_path.as_deref() {
        Some(path) => path.to_string(),
        None if content.is_empty() => return Ok(()),
        None => attach_new_note_file(state, &store, &doc)?,
    };

    crate::commands::file::authorize_source_write(state, &source_path)?;
    let stamp = ignore_stamper(state);
    let written = store
        .save_to_source_without_index(id, content, state.disk_state(id), Some(&stamp))
        .map_err(|e| save_failure_message(&e))?;
    state.set_disk_state(id, written);
    Ok(())
}

/// The hook the store calls immediately before each of its writes, which
/// stamps the file in the watcher's ignore set.
///
/// One key for the one file a write touches: that file's canonical path under
/// the source namespace. A save used to stamp the bare name as well, which is
/// a global key — a save of `a/index.md` suppressed a real change to
/// `b/index.md` for as long as the stamp lived, and collided with the config
/// watcher, which keyed on the bare `config.toml` (ADR-028 section 6).
///
/// The path is resolved as far as the filesystem knows it, so a file being
/// created is keyed by its canonical folder plus its name and the event that
/// creation produces still finds the stamp. Without a stamp the write comes
/// back as an external change: a config reload, or an arrival that reopens the
/// tab and pulls the window forward, on every keystroke.
///
/// It is handed to the store rather than run before the call because the store
/// writes more than the note: a save that cannot land writes the text it was
/// carrying beside the note, and that file lands in the same watched folder.
pub(crate) fn ignore_stamper(state: &AppState) -> impl Fn(&Path, &[u8]) + '_ {
    move |path: &Path, bytes: &[u8]| {
        let key =
            writ_core::watcher::ignore::source_key(&crate::watcher::handler::ignore_key_path(path));
        let mut ignore = recover_poison(
            state.watcher_ignore.lock(),
            "commands::buffer::ignore_stamper",
        );
        ignore.record(key, bytes, Instant::now());
    }
}

/// Gives a note with no file the file the invariant requires: a dated `.md`
/// in the notes folder, deduped Finder-style, writable by containment.
///
/// The write then falls through to the ordinary path, so exactly one code
/// path writes a note's text.
fn attach_new_note_file(
    state: &AppState,
    store: &BufferStore,
    doc: &BufferDocument,
) -> Result<String, String> {
    crate::notes::attach_note_file(
        store,
        &state.notes_root,
        &doc.id,
        &doc.title,
        chrono::Utc::now(),
    )
}

#[tauri::command]
pub fn save_buffer_content(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<(), String> {
    save_buffer_content_inner(&state, &id, &content)?;
    if let Some(generation) = state.fts_scheduler.on_edit(&id) {
        spawn_deferred_reindex(app, id, generation);
    }
    Ok(())
}

/// IPC: writes buffer content and leaves the search index alone.
///
/// The third-party notices listing used to be written through here after a
/// plain [`create_buffer`], which minted it a file in the notes folder like
/// any other note. It no longer is: a generated document opens read-only
/// through [`crate::commands::file::open_generated_document`] instead, whose
/// existing read-only refusal is what stops a save of one, so this command
/// no longer has a caller that reaches it with no file yet to mint one for
/// (ADR-028 §1). It is left in place as the unindexed counterpart of
/// [`save_buffer_content`], for whichever caller needs it next.
///
/// Destination follows the same rule as every other save: the file the note
/// lives in, minted in the notes folder when it has none.
#[tauri::command]
pub fn save_buffer_content_unindexed(
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<(), String> {
    save_buffer_content_inner(&state, &id, &content)
}

/// Spawns the worker that reindexes a buffer once its edits settle. The worker
/// waits one debounce window, and either reindexes (the buffer stopped
/// changing) or waits again (a newer edit arrived), so an edit burst collapses
/// to a single reindex of the latest on-disk content (ADR-020).
fn spawn_deferred_reindex(app: AppHandle, id: String, first_generation: u64) {
    tauri::async_runtime::spawn(async move {
        let mut seen = first_generation;
        loop {
            let _ = tauri::async_runtime::spawn_blocking(|| {
                std::thread::sleep(FTS_REINDEX_DEBOUNCE);
            })
            .await;
            let state = app.state::<AppState>();
            match state.fts_scheduler.poll(&id, seen) {
                PollOutcome::Wait(generation) => seen = generation,
                PollOutcome::Reindex => {
                    match state.store.lock() {
                        Ok(store) => {
                            if let Err(e) = store.reindex_buffer(&id) {
                                tracing::debug!(buffer_id = %id, error = %e, "deferred fts reindex failed");
                            }
                        }
                        Err(_) => {
                            tracing::debug!(buffer_id = %id, "store poisoned; skipping deferred reindex")
                        }
                    }
                    break;
                }
            }
        }
    });
}

/// Reads a note's text from its file, recording what the file held.
///
/// This read is what the reload of an externally changed note goes through,
/// so it is where the record of the file moves: an event the editor declined
/// must leave the record where it was, or the next reopen has nothing left to
/// tell the user about ([`crate::commands::file::resync_open_buffer`]).
///
/// A read-only row's displayed text is not always the file's bytes — a binary
/// file reads back as a hex dump — so its record comes from hashing the file
/// itself. Skipping it instead is what made a generated document emit a
/// change on its first reopen after a restart, having been read but never
/// recorded.
///
/// A note with no file yet has nothing on disk to record.
pub fn read_buffer_content_inner(state: &AppState, id: &str) -> Result<Vec<u8>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    let content = store.read_content(id).map_err(|e| e.to_string())?;
    if let Some(source_path) = doc.source_path.as_deref() {
        let path = Path::new(source_path);
        if doc.read_only {
            if let Ok(bytes) = std::fs::read(path) {
                state.record_disk_state_bytes(id, path, &bytes);
            }
        } else {
            state.record_disk_state_bytes(id, path, content.as_bytes());
        }
    }
    Ok(content.into_bytes())
}

/// IPC: [`read_buffer_content_inner`] as the frontend sees it.
///
/// Returns `tauri::ipc::Response` so the browser-side `invoke()` yields an
/// `ArrayBuffer` directly, bypassing JSON string-escaping. The caller
/// decodes with `new TextDecoder().decode(bytes)`.
#[tauri::command]
pub fn read_buffer_content(
    state: State<'_, AppState>,
    id: String,
) -> Result<tauri::ipc::Response, String> {
    Ok(tauri::ipc::Response::new(read_buffer_content_inner(
        &state, &id,
    )?))
}

#[tauri::command]
pub fn list_active_buffers(state: State<'_, AppState>) -> Result<Vec<BufferDocument>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .list_by_status(BufferStatus::Active)
        .map_err(|e| e.to_string())
}

/// Drops the stamp the note's last save left, so a stamp cannot outlive the
/// tab that made it.
///
/// A note that never reached a file has nothing stamped.
fn forget_ignore_stamp(state: &AppState, doc: &BufferDocument, context: &'static str) {
    let Some(path) = doc.source_path.as_deref() else {
        return;
    };
    let key = writ_core::watcher::ignore::source_key(&crate::watcher::handler::ignore_key_path(
        Path::new(path),
    ));
    let mut ignore = recover_poison(state.watcher_ignore.lock(), context);
    ignore.remove(&key);
}

/// IPC: closes one tab.
///
/// What the file held is forgotten with the tab. The record exists to answer
/// "did this change since Writ last looked", and after a close Writ has not
/// looked; keeping it would let a note edited elsewhere while its tab was
/// closed reopen without a word.
pub fn close_buffer_inner(state: &AppState, id: &str) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    forget_ignore_stamp(state, &doc, "commands::buffer::close_buffer");
    store.close(id).map_err(|e| e.to_string())?;
    state.forget_disk_state(id);
    Ok(())
}

#[tauri::command]
pub fn close_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    close_buffer_inner(&state, &id)
}

pub fn close_buffers_inner(state: &AppState, ids: &[String]) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.close_many(ids).map_err(|e| e.to_string())?;
    for id in ids {
        state.forget_disk_state(id);
    }
    Ok(())
}

#[tauri::command]
pub fn close_buffers(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    close_buffers_inner(&state, &ids)
}

pub fn delete_buffer_inner(state: &AppState, id: &str) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).ok();
    if let Some(doc) = doc.as_ref() {
        forget_ignore_stamp(state, doc, "commands::buffer::delete_buffer");
    }
    store.delete(id).map_err(|e| e.to_string())?;
    state.forget_disk_state(id);
    Ok(())
}

#[tauri::command]
pub fn delete_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    delete_buffer_inner(&state, &id)
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
