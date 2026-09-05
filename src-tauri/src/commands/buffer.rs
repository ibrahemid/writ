use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use crate::fts_scheduler::{PollOutcome, FTS_REINDEX_DEBOUNCE};
use crate::poison::recover_poison;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::buffer::manager::BufferManager;
use writ_core::notes::guard::is_not_downloaded;
use writ_core::watcher::pending::HoldAnswer;
use writ_storage::buffer_store::{BufferStore, NoteFileState};
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

/// Code a save carries when the file is reachable under more than one name.
pub const ERR_HARD_LINKED: &str = "ERR_HARD_LINKED";

/// Code a save or a rename carries when the file itself is marked read-only.
pub const ERR_READ_ONLY_DESTINATION: &str = "ERR_READ_ONLY_DESTINATION";

/// Code a save carries when the folder holding the file would not take the
/// write.
///
/// Apart from [`ERR_READ_ONLY_DESTINATION`] because the file is fine and the
/// folder is not, and because a folder can be writable again a moment later,
/// which is what makes this one worth pressing again.
pub const ERR_FOLDER_NOT_WRITABLE: &str = "ERR_FOLDER_NOT_WRITABLE";

/// Code a save carries when the note's file was deleted and nothing carrying
/// its identity was found.
///
/// Writing would recreate a file the user threw away, and in a synced folder
/// it would put it back on every device. The text stays in the tab and the
/// person decides where it goes (spec W4).
pub const ERR_FILE_REMOVED_ON_DISK: &str = "ERR_FILE_REMOVED_ON_DISK";

/// Code a save carries when the note itself is not writable, which a
/// generated document and a binary file both are.
pub const ERR_NOTE_READ_ONLY: &str = "ERR_NOTE_READ_ONLY";

/// Code a save carries when the filesystem refused the write.
pub const ERR_PERMISSION_DENIED: &str = "ERR_PERMISSION_DENIED";

/// Code a save carries when another program is holding the file open, which
/// on Windows stops the rename a save ends with.
pub const ERR_FILE_IN_USE: &str = "ERR_FILE_IN_USE";

/// Code a save carries when the file, or the folder above it, is gone.
pub const ERR_FILE_MISSING: &str = "ERR_FILE_MISSING";

/// Code a save carries when the filesystem stopped answering, which is what a
/// disconnected network volume looks like.
pub const ERR_WRITE_TIMED_OUT: &str = "ERR_WRITE_TIMED_OUT";

/// Code a save carries when the write failed for a reason with no sentence of
/// its own.
///
/// The message after the code is the operating system's, so the editor renders
/// the code and never the message: `Os error 28` and a note's id are the two
/// things a person must not be handed instead of an explanation.
pub const ERR_WRITE_FAILED: &str = "ERR_WRITE_FAILED";

/// Renders a failed save for the frontend as a stable code the editor writes
/// its own sentence from, followed by the message a log wants.
///
/// Every arm carries a code. A failure with no code would reach the editor as
/// whatever the layer beneath happened to say, which is where an errno or a
/// note's id gets shown to a person.
pub fn save_failure_message(error: &StorageError) -> String {
    format!("{}: {error}", failure_code(error))
}

/// The stable code alone, for a caller that reports the failure as a code
/// rather than as a sentence.
///
/// A propagated rename names each file it could not rewrite, and the reason
/// has to be a code the editor writes its own sentence from: the message
/// [`save_failure_message`] appends is the operating system's, and `Os error
/// 28` is one of the two things a person must not be handed instead of an
/// explanation.
pub fn failure_code(error: &StorageError) -> &'static str {
    match error {
        StorageError::SourceChangedOnDisk { .. } => ERR_FILE_CHANGED_ON_DISK,
        StorageError::SourceNotDownloaded { .. } => ERR_FILE_NOT_DOWNLOADED,
        StorageError::HardLinkedDestination { .. } => ERR_HARD_LINKED,
        StorageError::DestinationReadOnly { .. } => ERR_READ_ONLY_DESTINATION,
        StorageError::DestinationFolderNotWritable { .. } => ERR_FOLDER_NOT_WRITABLE,
        StorageError::Io(io) => io_failure_code(io.kind()),
        _ => ERR_WRITE_FAILED,
    }
}

/// The code for an `io::ErrorKind` a save can come back with.
///
/// Only kinds with something specific to tell the person get one of their own;
/// the rest share [`ERR_WRITE_FAILED`], whose sentence says what happened
/// without repeating the operating system's wording.
fn io_failure_code(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::PermissionDenied => ERR_PERMISSION_DENIED,
        std::io::ErrorKind::ResourceBusy => ERR_FILE_IN_USE,
        std::io::ErrorKind::NotFound => ERR_FILE_MISSING,
        std::io::ErrorKind::TimedOut => ERR_WRITE_TIMED_OUT,
        _ => ERR_WRITE_FAILED,
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
///
/// Returns what the file holds once the write has landed, so the tab can stop
/// reading dirty without going back to disk for the answer. A note with
/// nothing in it and no file yet wrote nothing and has no state to report.
pub fn save_buffer_content_inner(
    state: &AppState,
    id: &str,
    content: &str,
) -> Result<Option<String>, String> {
    write_note_source(state, id, content, RemovedFile::Refuse)
}

/// What a write does about a note whose file was deleted outside Writ.
///
/// A keystroke must not recreate it and an explicit request to put it back
/// must not be refused. Both are the same write with opposite answers to that
/// one question, so they are one function and this is the difference between
/// them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemovedFile {
    /// Refuse under `ERR_FILE_REMOVED_ON_DISK` (spec W4).
    Refuse,
    /// Write the file back at the path the note names.
    WriteBack,
}

/// The write every save lands through, with the removed-file question left to
/// the caller.
fn write_note_source(
    state: &AppState,
    id: &str,
    content: &str,
    removed: RemovedFile,
) -> Result<Option<String>, String> {
    wait_out_a_held_removal(state, id, removed)?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    if doc.read_only {
        return Err(format!("{ERR_NOTE_READ_ONLY}: note {id} is read-only"));
    }
    // A file the user deleted is not recreated by the next keystroke. The
    // write gate cannot answer this on its own: a path that is not there
    // resolves and passes, which is what lets a new note be minted and is
    // right everywhere except here (spec W4).
    if removed == RemovedFile::Refuse && state.is_removed_on_disk(id) {
        return Err(format!(
            "{ERR_FILE_REMOVED_ON_DISK}: note {id} has no file on disk any more"
        ));
    }

    let source_path = match doc.source_path.as_deref() {
        Some(path) => path.to_string(),
        None if content.is_empty() => return Ok(None),
        None => attach_new_note_file(state, &store, &doc)?,
    };

    crate::commands::file::authorize_source_write(state, &source_path)?;
    let stamp = ignore_stamper(state);
    let written = store
        .save_to_source_without_index(id, content, state.disk_state(id), Some(&stamp))
        .map_err(|e| save_failure_message(&e))?;
    state.set_disk_state(id, written);
    // Every save writes a temporary file and renames it over the note, so the
    // file behind this tab is a new file now. A record taken before the save
    // names one nothing carries any more, and the first delete after it would
    // read a move as a delete.
    state.refresh_source_identity(id, Path::new(&source_path));
    // The digest the editor compares its document against, not the guard's
    // raw one: the bytes just written are the document, so this is the answer
    // that makes the tab read clean.
    Ok(Some(writ_core::hash::comparison_digest_hex(
        content.as_bytes(),
    )))
}

/// Waits out a removal a watcher is still holding for this note.
///
/// A hold is the window in which nothing knows yet whether the note still has
/// a file: its path has gone empty, and the delivery that would carry the
/// other half of a rename may not have arrived. The record is left saying what
/// it said before for exactly that window, so there is nothing in it for a
/// write to trip over, and a write that lands in it recreates a file the
/// person deleted or leaves a second one behind a rename the tab never hears
/// about (ADR-033 §14).
///
/// So the write waits, and the answer decides: a move has already moved the
/// row, so the destination read after this is the new path; a removal has
/// already marked the note, so the refusal is word for word the one a save
/// after any announcement gets. A file back at its own path answers too, and
/// the write lands where it always would have.
///
/// [`RemovedFile::WriteBack`] waits the same and is refused by nothing: it is
/// the explicit request to put the file back, and an answer that arrives while
/// it waits is what tells it where to put it.
///
/// Before the store lock, and holding nothing itself: answering a hold moves
/// the note's row, which needs that same lock.
///
/// The wait is bounded by the hold's own deadline, so a watcher that stopped
/// while holding one costs a write that window and no more.
fn wait_out_a_held_removal(state: &AppState, id: &str, removed: RemovedFile) -> Result<(), String> {
    match state.removal_holds.wait_for_answer(id) {
        Some(HoldAnswer::Removed) if removed == RemovedFile::Refuse => Err(format!(
            "{ERR_FILE_REMOVED_ON_DISK}: note {id} has no file on disk any more"
        )),
        _ => Ok(()),
    }
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
///
/// The tab starts following the file here rather than in the caller: this is
/// the moment the note gets one, and a note that reached its file this way and
/// was never followed heard nothing about it for the rest of the session.
fn attach_new_note_file(
    state: &AppState,
    store: &BufferStore,
    doc: &BufferDocument,
) -> Result<String, String> {
    let path = crate::notes::attach_note_file(
        store,
        &state.notes_root(),
        &doc.id,
        &doc.title,
        chrono::Utc::now(),
    )?;
    state.follow_note_path(&doc.id, Path::new(&path));
    Ok(path)
}

#[tauri::command]
pub fn save_buffer_content(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<Option<String>, String> {
    let disk_hash = save_buffer_content_inner(&state, &id, &content)?;
    if let Some(generation) = state.fts_scheduler.on_edit(&id) {
        spawn_deferred_reindex(app, id, generation);
    }
    Ok(disk_hash)
}

/// Writes a note whose file was deleted outside Writ back to the path it
/// names.
///
/// [`save_buffer_content_inner`] refuses that write, and has to: the tab's
/// text is the last copy of the note (ADR-028 §1), so every keystroke would
/// otherwise put back a file the person threw away, on every synced device.
/// Asking for it is the other half of the same rule — the text is the person's
/// and this is where they say it goes back.
///
/// The record is reset from the file only after the write lands. Clearing it
/// first would leave a failed restore unmarked, autosave unblocked, and the
/// next keystroke recreating the file silently. A folder that is gone fails
/// here like any other write and comes back under `ERR_FILE_MISSING`.
pub fn restore_note_file_inner(
    state: &AppState,
    id: &str,
    content: &str,
) -> Result<Option<String>, String> {
    let disk_hash = write_note_source(state, id, content, RemovedFile::WriteBack)?;
    let source_path = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.get(id).map_err(|e| e.to_string())?.source_path
    };
    if let Some(path) = source_path {
        state.observe_source_file(id, Path::new(&path));
    }
    Ok(disk_hash)
}

/// IPC: [`restore_note_file_inner`].
#[tauri::command]
pub fn restore_note_file(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<Option<String>, String> {
    let disk_hash = restore_note_file_inner(&state, &id, &content)?;
    if let Some(generation) = state.fts_scheduler.on_edit(&id) {
        spawn_deferred_reindex(app, id, generation);
    }
    Ok(disk_hash)
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
) -> Result<Option<String>, String> {
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
            // The reload of an externally changed note comes through here, so
            // a file that gained or lost its carriage returns while Writ had
            // it open is followed rather than written back the old way.
            let ending = writ_core::notes::line_ending::LineEnding::detect(&content);
            if ending != doc.line_ending {
                if let Err(e) = store.set_line_ending(id, ending) {
                    tracing::debug!(buffer_id = %id, error = %e, "the file's line ending could not be recorded");
                }
            }
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

/// What a note's file holds right now, as the editor reads it.
///
/// `hash` is the only field a decision may rest on; `size` and `mtime_ms` are
/// carried for the same diagnostic reasons [`DiskState`] carries them.
///
/// It is the comparison digest ([`writ_core::hash::comparison_digest_hex`]),
/// not the write guard's raw one: this is the number the editor holds the
/// document up against, and the document's line endings are CodeMirror's
/// rather than the file's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiskStateDto {
    pub hash: String,
    pub size: u64,
    pub mtime_ms: Option<i64>,
}

impl From<NoteFileState> for DiskStateDto {
    fn from(state: NoteFileState) -> Self {
        Self {
            hash: state.comparison_hash,
            size: state.disk.size,
            mtime_ms: state.disk.mtime.and_then(|mtime| {
                mtime
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|since| i64::try_from(since.as_millis()).ok())
            }),
        }
    }
}

/// What Writ can say about the file behind a note.
///
/// The three answers are kept apart because the editor acts on them
/// differently and collapsing them loses text. A note with no file yet is a
/// new note: nothing is known to differ, so it reads clean. A note whose row
/// names a file that cannot be described is the same situation as this call
/// failing outright — the editor holds a document with nothing to compare it
/// against — and the dirty predicate has to fail closed for it, or a later
/// reload replaces text no file holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum NoteDiskAnswer {
    /// The file was read, and `disk` describes it.
    Described { disk: DiskStateDto },
    /// The note has no file: nothing has saved it yet.
    NoFile,
    /// The note names a file and nothing could be said about it — it is not
    /// there, or its bytes are not on this machine.
    Undescribed,
}

/// [`note_disk_state`] once the file's path and flags are known.
///
/// A file whose bytes are not on this machine is reported undescribed without
/// being read: reading it makes the provider daemon fetch it over the network
/// (ADR-028 §5), which is the one thing a re-check after a failed save must
/// not set off.
fn note_disk_state_of(path: &Path, st_flags: Option<u32>) -> Result<NoteDiskAnswer, String> {
    if is_not_downloaded(st_flags) {
        return Ok(NoteDiskAnswer::Undescribed);
    }
    Ok(writ_storage::buffer_store::read_note_file_state(path)
        .map_err(|e| e.to_string())?
        .map(|state| NoteDiskAnswer::Described {
            disk: DiskStateDto::from(state),
        })
        .unwrap_or(NoteDiskAnswer::Undescribed))
}

/// IPC: what the file behind note `id` holds now.
///
/// The tab whose save was stopped is the caller: the reason it was stopped is
/// a difference between the file and what Writ last read, and this is how the
/// tab reads that difference again after the person has looked at it. The
/// editor also asks on open, to take the file's digest from the side that
/// read the file.
///
/// The path is resolved from the note's row rather than passed in, for the
/// same reason a save's destination is ([`save_buffer_content_inner`]).
#[tauri::command]
pub fn note_disk_state(state: State<'_, AppState>, id: String) -> Result<NoteDiskAnswer, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(&id).map_err(|e| e.to_string())?;
    let Some(source_path) = doc.source_path.as_deref() else {
        return Ok(NoteDiskAnswer::NoFile);
    };
    let path = Path::new(source_path);
    note_disk_state_of(path, writ_storage::buffer_store::dataless_flags(path))
}

/// One note's text as the editor holds it, for a save that could not land.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UnsavedNote {
    pub id: String,
    pub content: String,
}

/// IPC: keeps text a save could not write until the shutdown snapshot takes
/// it (spec S2, the quit-with-a-failed-save case).
///
/// Writ is quitting by the time this is called and the file is the one place
/// the text could not go, so the snapshot is where it goes instead; the next
/// launch restores it ([`crate::finish_shutdown`]).
#[tauri::command]
pub fn record_unsaved_notes(
    state: State<'_, AppState>,
    notes: Vec<UnsavedNote>,
) -> Result<(), String> {
    for note in notes {
        state.record_unsaved_on_exit(&note.id, note.content);
    }
    Ok(())
}

/// IPC: the tabs the last session left open.
///
/// This is also where a restored tab starts being followed. A tab nobody has
/// brought to the front is the one most likely to be sitting on a file that
/// moved on, so waiting for the user to click it would leave the whole
/// restored session unwatched.
#[tauri::command]
pub fn list_active_buffers(state: State<'_, AppState>) -> Result<Vec<BufferDocument>, String> {
    let docs = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .list_by_status(BufferStatus::Active)
            .map_err(|e| e.to_string())?
    };
    for doc in &docs {
        state.follow_note_file(doc);
    }
    Ok(docs)
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
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let doc = store.get(id).map_err(|e| e.to_string())?;
        forget_ignore_stamp(state, &doc, "commands::buffer::close_buffer");
        store.close(id).map_err(|e| e.to_string())?;
    }
    state.forget_disk_state(id);
    state.forget_source_record(id);
    state.stop_following_note(id);
    Ok(())
}

#[tauri::command]
pub fn close_buffer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    close_buffer_inner(&state, &id)
}

pub fn close_buffers_inner(state: &AppState, ids: &[String]) -> Result<(), String> {
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.close_many(ids).map_err(|e| e.to_string())?;
    }
    for id in ids {
        state.forget_disk_state(id);
        state.forget_source_record(id);
        state.stop_following_note(id);
    }
    Ok(())
}

#[tauri::command]
pub fn close_buffers(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    close_buffers_inner(&state, &ids)
}

pub fn delete_buffer_inner(state: &AppState, id: &str) -> Result<(), String> {
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let doc = store.get(id).ok();
        if let Some(doc) = doc.as_ref() {
            forget_ignore_stamp(state, doc, "commands::buffer::delete_buffer");
        }
        store.delete(id).map_err(|e| e.to_string())?;
    }
    state.forget_disk_state(id);
    state.forget_source_record(id);
    state.stop_following_note(id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use writ_core::notes::guard::{DiskState, SF_DATALESS};

    /// The description in a `Described` answer, or a panic naming what came
    /// back instead.
    fn described(answer: NoteDiskAnswer) -> DiskStateDto {
        match answer {
            NoteDiskAnswer::Described { disk } => disk,
            other => panic!("expected a described file, got {other:?}"),
        }
    }

    #[test]
    fn a_file_that_is_not_there_is_undescribed_rather_than_fileless() {
        // Undescribed, not `NoFile`: the note names a file and Writ cannot
        // say what it holds, which is the answer the editor has to fail
        // closed on. `NoFile` means a new note, and reads clean.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone.md");
        assert_eq!(
            note_disk_state_of(&missing, None).unwrap(),
            NoteDiskAnswer::Undescribed
        );
    }

    #[test]
    fn a_normal_file_reports_the_digest_and_the_length_of_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, b"one line\n").unwrap();

        let state = described(note_disk_state_of(&path, None).unwrap());
        assert_eq!(state.hash, writ_core::hash::sha256_hex(b"one line\n"));
        assert_eq!(state.size, 9);
        assert!(state.mtime_ms.is_some());
    }

    #[test]
    fn a_file_that_is_not_downloaded_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("evicted.md");
        std::fs::write(&path, b"placeholder").unwrap();

        assert_eq!(
            note_disk_state_of(&path, Some(SF_DATALESS)).unwrap(),
            NoteDiskAnswer::Undescribed
        );
    }

    #[test]
    fn a_crlf_file_reports_the_digest_the_editor_will_compute() {
        // CodeMirror converts the line endings on load, so the document the
        // editor hashes is the LF form. Reporting the raw digest here would
        // make every CRLF file read as changed on disk for ever.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("windows.md");
        std::fs::write(&path, b"one\r\ntwo\r\n").unwrap();

        let state = described(note_disk_state_of(&path, None).unwrap());
        assert_eq!(state.hash, writ_core::hash::sha256_hex(b"one\ntwo\n"));
        assert_ne!(state.hash, writ_core::hash::sha256_hex(b"one\r\ntwo\r\n"));
        // The length is the file's, not the normalised text's.
        assert_eq!(state.size, 10);
    }

    #[test]
    fn the_three_answers_carry_a_state_the_frontend_can_branch_on() {
        // The editor's fail-closed rule rests on telling a new note apart
        // from a file nobody could read, so the tag has to survive
        // serialisation and the two undescribable cases must not share a
        // name with the fileless one.
        let described = serde_json::to_value(NoteDiskAnswer::Described {
            disk: DiskStateDto {
                hash: "abc".to_string(),
                size: 3,
                mtime_ms: None,
            },
        })
        .unwrap();
        assert_eq!(described["state"], "described");
        assert_eq!(described["disk"]["hash"], "abc");

        assert_eq!(
            serde_json::to_value(NoteDiskAnswer::NoFile).unwrap()["state"],
            "no_file"
        );
        assert_eq!(
            serde_json::to_value(NoteDiskAnswer::Undescribed).unwrap()["state"],
            "undescribed"
        );
    }

    fn file_state(
        hash: writ_core::hash::Sha256Digest,
        size: u64,
        mtime: Option<SystemTime>,
    ) -> NoteFileState {
        NoteFileState {
            comparison_hash: writ_core::hash::digest_hex(hash),
            disk: DiskState { hash, size, mtime },
        }
    }

    #[test]
    fn the_dto_renders_the_digest_as_hex_and_the_time_in_milliseconds() {
        let dto = DiskStateDto::from(file_state(
            writ_core::hash::sha256_bytes(b"writ"),
            4,
            Some(UNIX_EPOCH + std::time::Duration::from_millis(1_500)),
        ));
        assert_eq!(dto.hash, writ_core::hash::sha256_hex(b"writ"));
        assert_eq!(dto.size, 4);
        assert_eq!(dto.mtime_ms, Some(1_500));
    }

    #[test]
    fn a_time_the_filesystem_could_not_report_is_carried_as_none() {
        let dto = DiskStateDto::from(file_state(writ_core::hash::sha256_bytes(b""), 0, None));
        assert_eq!(dto.mtime_ms, None);
    }
}
