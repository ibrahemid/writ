use std::borrow::Cow;
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use crate::commands::note_index::{announce_indexed, holds_own_write, index_copy};
use crate::commands::offload::{note_ticket, run_blocking, run_in_turn, ERR_COMMAND_STOPPED};
use crate::fts_scheduler::{PollOutcome, FTS_REINDEX_DEBOUNCE};
use crate::poison::recover_poison;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::buffer::manager::BufferManager;
use writ_core::notes::guard::is_not_downloaded;
use writ_core::notes::line_ending::LineEnding;
use writ_core::notes::reload::{apply_choice, Action, ChangeChoice, Side};
use writ_core::recovery::{journal_step, JournalStep, SaveLanding};
use writ_core::serial::Turn;
use writ_core::text_encoding::TextEncoding;
use writ_core::watcher::pending::HoldAnswer;
use writ_storage::buffer_store::{dataless_flags, BufferStore, NoteFileState, SourceWrite};
use writ_storage::errors::StorageError;
use writ_storage::guarded::write_conflict_copy;

/// The one place a change from outside a write reaches the version store.
///
/// A reload is not a write, so it never reaches the facade's hook
/// (`writ_storage::guarded::history_hook`), and neither does a conflict
/// resolution or a file another program overwrote while Writ had it open.
/// Those are this seam, and between them the two are the whole set: nothing
/// captures inside the editor, inside a store, or at a third writer.
///
/// What this buys is the case Obsidian's file recovery does not cover
/// (spec 481): the text a note held before something else wrote over it is
/// kept, and it is kept before the tab takes the new one.
///
/// The merge window never applies here. It exists to collapse a run of Writ's
/// own saves, each of which leaves its text in the file for the next one to
/// replace; every text this seam sees is one Writ did not write and cannot
/// produce again, and two of them inside ten seconds is two programs writing,
/// not one person typing. There is no storm to collapse either: a text the
/// store already holds is deduped by its digest, so a tab reopened on a file
/// nothing touched costs nothing.
///
/// A note the notes folder does not hold is not versioned, and neither is a
/// row whose text is not the file's — a binary read back as a hex dump is not
/// a version of anything. A failure is logged and swallowed: a reload that
/// could not be recorded is still a reload.
pub(crate) fn keep_what_was_seen(state: &AppState, path: &Path, bytes: &[u8]) {
    let Some(note) = state.note_history.key_for(path) else {
        return;
    };
    let now = std::time::SystemTime::now();
    if let Err(e) = state.note_history.capture_replaced(&note, bytes, now) {
        tracing::warn!(
            note = %writ_storage::paths::file_name_only(&path.to_string_lossy()),
            error = %e,
            "this version of the note could not be kept"
        );
    }
}

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

/// Code a save carries when the note's text holds characters its file's
/// encoding has no bytes for, so nothing was written (ADR-049).
///
/// Unlike the other codes, what follows it is read too: the encoding's name,
/// ` has no bytes for `, and the characters as code points separated by
/// spaces (`ERR_ENCODING_LOSSY: windows-1252 has no bytes for U+1F600
/// U+4E2D`). The editor names the characters from them
/// (`src/lib/save-error.ts`).
pub const ERR_ENCODING_LOSSY: &str = "ERR_ENCODING_LOSSY";

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
        StorageError::EncodingLossy { .. } => ERR_ENCODING_LOSSY,
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

/// Code a read carries when the file's bytes are not UTF-8 text.
///
/// The read codes are the same contract as the save codes: the editor writes
/// its own sentence from the code, and the message after it is for logs.
pub const ERR_READ_NOT_UTF8: &str = "ERR_READ_NOT_UTF8";

/// Code a read carries when the file's bytes are not text in the encoding
/// other than UTF-8 that the note reads it in (ADR-049).
pub const ERR_READ_NOT_IN_ENCODING: &str = "ERR_READ_NOT_IN_ENCODING";

/// Code a read carries when the filesystem refused it.
pub const ERR_READ_PERMISSION_DENIED: &str = "ERR_READ_PERMISSION_DENIED";

/// Code a read carries when another program is holding the file, which on
/// Windows is a sharing lock that a second attempt can get past.
pub const ERR_READ_FILE_IN_USE: &str = "ERR_READ_FILE_IN_USE";

/// Code a read carries when the file, or the folder above it, is gone.
pub const ERR_READ_FILE_MISSING: &str = "ERR_READ_FILE_MISSING";

/// Code a read carries when the filesystem stopped answering, which is what a
/// disconnected network volume looks like.
pub const ERR_READ_TIMED_OUT: &str = "ERR_READ_TIMED_OUT";

/// Code a read carries when it failed for a reason with no sentence of its own.
pub const ERR_READ_FAILED: &str = "ERR_READ_FAILED";

/// Renders a failed read for the frontend as a stable code followed by the
/// message a log wants ([`save_failure_message`] is the same shape).
pub fn read_failure_message(error: &StorageError) -> String {
    format!("{}: {error}", read_failure_code(error))
}

/// The stable code for a failed read.
///
/// A file read in its encoding that is not text in it carries the code for
/// that (ADR-049): UTF-8's own, or the one for every other encoding.
/// `InvalidData` is what a reader that wanted UTF-8 reports for bytes that
/// are not. A file past the size
/// Writ opens carries the code an open of it answers with
/// ([`writ_core::file_ops::ERR_OPEN_TOO_LARGE`]); the size the editor states
/// is on the row, which [`read_buffer_content_inner`] records.
pub fn read_failure_code(error: &StorageError) -> &'static str {
    match error {
        StorageError::NotInEncoding { encoding } if encoding.name() == "UTF-8" => ERR_READ_NOT_UTF8,
        StorageError::NotInEncoding { .. } => ERR_READ_NOT_IN_ENCODING,
        StorageError::Io(io) => io_read_failure_code(io.kind()),
        StorageError::FileTooLarge { .. } => writ_core::file_ops::ERR_OPEN_TOO_LARGE,
        _ => ERR_READ_FAILED,
    }
}

/// The stable code for a read the filesystem failed, by the kind of failure.
///
/// Shared by the editor's read and the open's: both read the same file, so a
/// person is told the same thing about it from either.
pub fn io_read_failure_code(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::InvalidData => ERR_READ_NOT_UTF8,
        std::io::ErrorKind::PermissionDenied => ERR_READ_PERMISSION_DENIED,
        std::io::ErrorKind::ResourceBusy => ERR_READ_FILE_IN_USE,
        std::io::ErrorKind::NotFound => ERR_READ_FILE_MISSING,
        std::io::ErrorKind::TimedOut => ERR_READ_TIMED_OUT,
        _ => ERR_READ_FAILED,
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
///
/// A save that does not land leaves its text in the unsaved-text journal
/// before it returns, and one that lands clears the tab's entry
/// ([`keep_unsaved_text`], ADR-044).
///
/// Waits its turn in the note's queue ([`AppState::buffer_writes`]), so a save
/// never runs beside another write of the same note.
pub fn save_buffer_content_inner(
    state: &AppState,
    id: &str,
    content: &str,
) -> Result<Option<String>, String> {
    let turn = state.buffer_writes.turn(id);
    save_in_turn(state, &turn, content)
}

/// [`save_buffer_content_inner`] for a caller already holding the note's turn.
pub fn save_in_turn(
    state: &AppState,
    turn: &Turn,
    content: &str,
) -> Result<Option<String>, String> {
    write_note_source(state, turn, content, RemovedFile::Refuse)
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

/// Why a write did not land, before it is rendered for the frontend.
struct SaveRefused {
    /// What became of the text the write carried, which decides what the
    /// journal does ([`keep_unsaved_text`]).
    landing: SaveLanding,
    /// The coded message the frontend reads.
    message: String,
    /// The path the write minted for the note's first file, which the journal
    /// step takes back when nothing was made at it ([`take_back_minted_file`]).
    minted: Option<String>,
}

impl SaveRefused {
    /// A refusal that left the text with the editor alone.
    fn not_landed(message: String) -> Self {
        Self {
            landing: SaveLanding::NotLanded,
            message,
            minted: None,
        }
    }

    /// A refusal from the store, which knows whether the text went beside the
    /// note ([`StorageError::save_landing`]).
    fn storage(error: &StorageError) -> Self {
        Self {
            landing: error.save_landing(),
            message: save_failure_message(error),
            minted: None,
        }
    }

    /// This refusal, from a write that minted `path` for the note's first
    /// file.
    fn with_minted(self, path: String) -> Self {
        Self {
            minted: Some(path),
            ..self
        }
    }
}

/// The write every save lands through, with the removed-file question left to
/// the caller.
fn write_note_source(
    state: &AppState,
    turn: &Turn,
    content: &str,
    removed: RemovedFile,
) -> Result<Option<String>, String> {
    let mut set_aside = Vec::new();
    let written = write_note_source_unannounced(state, turn, content, removed, &mut set_aside);
    announce_indexed(&state.event_bus, set_aside);
    written.map_err(|refused| refused.message)
}

/// [`write_note_source`], leaving the announcement to the caller.
///
/// A write the guard stops sets the text aside in a copy, which is indexed
/// here and added to `set_aside` rather than announced, so a caller that wrote
/// more than this note announces all of it as one event (ADR-036 section 4).
///
/// Every way out of it reaches the journal, which is why the write itself is
/// one function and this the only caller: a refusal that returned early and
/// skipped the journal would be text a crash could lose.
///
/// A refusal comes back saying what became of the text, so a caller that
/// journals its own outcome as well (`Keep mine`, [`answer_external_change`])
/// records what this write did with it.
fn write_note_source_unannounced(
    state: &AppState,
    turn: &Turn,
    content: &str,
    removed: RemovedFile,
    set_aside: &mut Vec<String>,
) -> Result<Option<String>, SaveRefused> {
    let id = turn.key();
    let written = land_note_source(state, id, content, removed, set_aside);
    let (landing, minted) = match &written {
        Ok(_) => (SaveLanding::Landed, None),
        Err(refused) => (refused.landing, refused.minted.as_deref()),
    };
    keep_unsaved_text(state, id, content, landing, minted);
    written
}

/// Records or clears the unsaved-text journal entry of note `id`'s tab, as the
/// outcome of a write that carried `content` decides
/// ([`writ_core::recovery::journal_step`], ADR-044).
///
/// Runs after the write has let go of the store, and a journal that cannot be
/// written is a line in the log rather than a second error for the frontend:
/// the write's own answer is the one the tab acts on, and the editor still
/// holds the text and hands it over again on the way out.
///
/// `minted` is the path a refused write minted for the note's first file. It
/// comes off the row once the journal holds the text, under the same hold of
/// the store ([`take_back_minted_file`]).
///
/// Only the tab's own entry is touched. Text an earlier tab of the note handed
/// over, or a launch could not write, was released when the note's tab
/// stopped being that tab, and no save of this one carries it (ADR-044 §3).
/// Within the tab, the frontend writes one note at a time and hands text over
/// only after its flush, so no write of this note is still running when a
/// later one lands or a handover arrives.
///
/// Text written beside the note is remembered, so a handover of the same text
/// later in the session does not journal a second copy of it
/// ([`record_unsaved_notes_inner`]).
pub(crate) fn keep_unsaved_text(
    state: &AppState,
    id: &str,
    content: &str,
    landing: SaveLanding,
    minted: Option<&str>,
) {
    let step = journal_step(landing);
    if step == JournalStep::Leave {
        return;
    }
    let mut store = recover_poison(state.store.lock(), "commands::buffer::keep_unsaved_text");
    if landing == SaveLanding::WrittenBeside {
        store.remember_text_written_beside(id, content);
    }
    let held = match step {
        JournalStep::Record => store.record_unsaved_text(id, content, chrono::Utc::now()),
        JournalStep::Clear => store.clear_unsaved_text(id).map(|_| false),
        JournalStep::Leave => Ok(false),
    };
    match held {
        Ok(true) => {
            if let Some(path) = minted {
                take_back_minted_file(state, &store, id, path);
            }
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(buffer_id = %id, error = %e, "the unsaved-text journal could not be updated");
        }
    }
}

/// Takes the path a refused write minted for note `id`'s first file back off
/// its row, now that the journal holds the text.
///
/// A path on a row says the note has a file, and a relaunch reads a path with
/// nothing at it as a file somebody deleted: the text would come back to its
/// tab as deleted and no file would be made (ADR-044 §4). Without the path the
/// note is one that never had a file, which the relaunch gives one, and the
/// next save mints again. The journal entry is what keeps such a note from
/// being reused for a new tab or reclaimed at the launch, so a text that could
/// not be recorded leaves the path where it is.
///
/// The tab stops following the path with it. A path something is at stays
/// ([`BufferStore::detach_source_path_with_no_file`]).
fn take_back_minted_file(state: &AppState, store: &BufferStore, id: &str, path: &str) {
    match store.detach_source_path_with_no_file(id, path) {
        Ok(true) => {
            state.stop_following_note(id);
            state.forget_source_record(id);
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(buffer_id = %id, error = %e, "a minted path could not be taken back");
        }
    }
}

/// [`write_note_source`]'s write, answering with what became of the text when
/// it did not land.
fn land_note_source(
    state: &AppState,
    id: &str,
    content: &str,
    removed: RemovedFile,
    set_aside: &mut Vec<String>,
) -> Result<Option<String>, SaveRefused> {
    wait_out_a_held_removal(state, id, removed).map_err(SaveRefused::not_landed)?;
    // Read before the store lock, and unconditionally: the config lock is
    // never taken under a held store lock, and finding out whether this write
    // needs the format at all takes the store lock first.
    let default_extension = state.default_extension();
    let Some(PlannedNoteWrite {
        write,
        source_path,
        minted,
    }) = plan_note_write(state, id, content, removed, default_extension)?
    else {
        return Ok(None);
    };
    let carry_minted = |refused: SaveRefused| {
        if minted {
            refused.with_minted(source_path.clone())
        } else {
            refused
        }
    };

    // The file is written without the store (ADR-047). The note's turn is
    // what keeps every other write and recorded read of it out until the
    // record below describes what landed.
    let stamp = ignore_stamper(state);
    let outcome = match write.land(state.disk_state(id), Some(&stamp)) {
        Ok(outcome) => outcome,
        Err(error) => {
            if let StorageError::SourceChangedOnDisk {
                conflict_copy: Some(copy),
                ..
            } = &error
            {
                set_aside.extend(index_copy(state, Path::new(copy)));
            }
            return Err(carry_minted(SaveRefused::storage(&error)));
        }
    };
    // The record and the opt-in move under one hold of the store, which a
    // render's scripts check takes too: it never sees the new bytes recorded
    // beside an opt-in that has not followed them yet.
    let settled = {
        let store = recover_poison(state.store.lock(), "commands::buffer::land_note_source");
        let settled = store.settle_source_write(id, &outcome);
        state.set_disk_state(id, outcome.disk_state);
        settled
    };
    // Every save writes a temporary file and renames it over the note, so the
    // file behind this tab is a new file now. A record taken before the save
    // names one nothing carries any more, and the first delete after it would
    // read a move as a delete.
    state.refresh_source_identity(id, Path::new(&source_path));
    settled.map_err(|error| carry_minted(SaveRefused::storage(&error)))?;
    // The digest the editor compares its document against, not the guard's
    // raw one: the bytes just written are the document, so this is the answer
    // that makes the tab read clean.
    Ok(Some(writ_core::hash::comparison_digest_hex(
        content.as_bytes(),
    )))
}

/// A write of a note's text, planned under the store: what to write, where,
/// and whether that file was minted for it.
struct PlannedNoteWrite<'a> {
    write: SourceWrite<'a>,
    source_path: String,
    minted: bool,
}

/// Everything [`land_note_source`] needs the store for before the file is
/// written, under one hold of it: the row's checks, the first file a note with
/// none is given, the write gate, and the bytes the file will hold.
///
/// `None` for a note with no file and no text, which has nothing to write.
fn plan_note_write<'a>(
    state: &AppState,
    id: &str,
    content: &'a str,
    removed: RemovedFile,
    default_extension: writ_core::config::FileExtension,
) -> Result<Option<PlannedNoteWrite<'a>>, SaveRefused> {
    let store = state
        .store
        .lock()
        .map_err(|e| SaveRefused::not_landed(e.to_string()))?;
    let doc = store
        .get(id)
        .map_err(|e| SaveRefused::not_landed(e.to_string()))?;
    if doc.read_only {
        return Err(SaveRefused {
            landing: SaveLanding::ReadOnlyNote,
            message: format!("{ERR_NOTE_READ_ONLY}: file {id} is read-only"),
            minted: None,
        });
    }
    // A file the user deleted is not recreated by the next keystroke. The
    // write gate cannot answer this on its own: a path that is not there
    // resolves and passes, which is what lets a new note be minted and is
    // right everywhere except here (spec W4).
    if removed == RemovedFile::Refuse && state.is_removed_on_disk(id) {
        return Err(SaveRefused::not_landed(format!(
            "{ERR_FILE_REMOVED_ON_DISK}: file {id} is on disk nowhere any more"
        )));
    }

    let (source_path, minted) = match doc.source_path.as_deref() {
        Some(path) => (path.to_string(), false),
        None if content.is_empty() => return Ok(None),
        None => (
            attach_new_note_file(state, &store, &doc, default_extension)
                .map_err(SaveRefused::not_landed)?,
            true,
        ),
    };

    let carry_minted = |refused: SaveRefused| {
        if minted {
            refused.with_minted(source_path.clone())
        } else {
            refused
        }
    };
    crate::commands::file::authorize_source_write(state, &source_path)
        .map_err(|message| carry_minted(SaveRefused::not_landed(message)))?;
    let write = store
        .plan_source_write(id, content)
        .map_err(|error| carry_minted(SaveRefused::storage(&error)))?;
    Ok(Some(PlannedNoteWrite {
        write,
        source_path,
        minted,
    }))
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
            "{ERR_FILE_REMOVED_ON_DISK}: file {id} is on disk nowhere any more"
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

/// Gives a note with no file the file the invariant requires: a dated file in
/// the configured format, in the notes folder, deduped by counter, writable
/// by containment.
///
/// The write then falls through to the ordinary path, so exactly one code
/// path writes a note's text.
///
/// The tab starts following the file here rather than in the caller: this is
/// the moment the note gets one, and a note that reached its file this way and
/// was never followed heard nothing about it for the rest of the session.
///
/// The format is handed in rather than read here: the caller holds the store
/// lock, and taking the config lock under it is the nesting order to keep out
/// of the file.
fn attach_new_note_file(
    state: &AppState,
    store: &BufferStore,
    doc: &BufferDocument,
    default_extension: writ_core::config::FileExtension,
) -> Result<String, String> {
    let path = crate::notes::attach_note_file(
        store,
        &state.notes_root(),
        &doc.id,
        &doc.title,
        chrono::Utc::now(),
        default_extension,
    )?;
    state.follow_note_path(&doc.id, Path::new(&path));
    Ok(path)
}

/// IPC: [`save_buffer_content_inner`], off the main thread and in the note's
/// queue (ADR-047).
#[tauri::command]
pub async fn save_buffer_content<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    content: String,
) -> Result<Option<String>, String> {
    let ticket = note_ticket(&app, &id);
    let reindex = app.clone();
    run_in_turn(app, ticket, ERR_WRITE_FAILED, move |state, turn| {
        let disk_hash = save_in_turn(state, turn, &content)?;
        schedule_reindex(&reindex, state, turn.key());
        Ok(disk_hash)
    })
    .await
}

/// Schedules the deferred reindex of note `id` after a write, unless the
/// index takes nothing from its file: a file over the normal tier, or a
/// read-only one, is never read for it (EP-15).
fn schedule_reindex<R: Runtime>(app: &AppHandle<R>, state: &AppState, id: &str) {
    let indexed = match state.store.lock() {
        Ok(store) => matches!(store.reindex_target(id), Ok(Some(_))),
        Err(_) => false,
    };
    if !indexed {
        return;
    }
    if let Some(generation) = state.fts_scheduler.on_edit(id) {
        spawn_deferred_reindex(app.clone(), id.to_string(), generation);
    }
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
    let turn = state.buffer_writes.turn(id);
    restore_in_turn(state, &turn, content)
}

/// [`restore_note_file_inner`] for a caller already holding the note's turn.
fn restore_in_turn(state: &AppState, turn: &Turn, content: &str) -> Result<Option<String>, String> {
    let id = turn.key();
    let disk_hash = write_note_source(state, turn, content, RemovedFile::WriteBack)?;
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
pub async fn restore_note_file<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    content: String,
) -> Result<Option<String>, String> {
    let ticket = note_ticket(&app, &id);
    let reindex = app.clone();
    run_in_turn(app, ticket, ERR_WRITE_FAILED, move |state, turn| {
        let disk_hash = restore_in_turn(state, turn, &content)?;
        schedule_reindex(&reindex, state, turn.key());
        Ok(disk_hash)
    })
    .await
}

/// Gives a note with no file its first one at `path`, a place the person
/// chose, and writes the tab's text there: "Save As…" on the bar of a note
/// whose first file could not be made in the notes folder (ADR-044 §4).
///
/// `path` has to be one the write gate allows: inside the notes folder, or
/// one the person picked in the save panel
/// ([`crate::commands::file::pick_note_save_path`]). A note that has a file
/// is refused rather than moved, and so is a file another tab has open: a
/// file is one open note, and two tabs on it would each save over the other.
/// `path` is resolved before anything compares it with a row, so another
/// spelling of a file (a link, `/var` for `/private/var`) is that file, and
/// the row records the resolved spelling.
///
/// A file already at `path` was there when the person chose it, and the save
/// panel asked before replacing it. Writ holds no record of that file, so the
/// guard lets the write through and keeps the text it replaces as a version,
/// as for any write it lets through against no record
/// ([`writ_core::notes::guard::decide_save`]). A file whose bytes are not on
/// this machine is refused by the guard without being read.
///
/// The write is the one every save lands through ([`restore_note_file_inner`]),
/// so the tab's journal entry clears when it lands and keeps the text when it
/// does not. A write that does not land takes the path back off the row: the
/// note had no file before, and has none after.
pub fn save_note_to_inner(
    state: &AppState,
    id: &str,
    content: &str,
    path: &str,
) -> Result<Option<String>, String> {
    let turn = state.buffer_writes.turn(id);
    save_note_to_in_turn(state, &turn, content, path)
}

/// [`save_note_to_inner`] for a caller already holding the note's turn.
fn save_note_to_in_turn(
    state: &AppState,
    turn: &Turn,
    content: &str,
    path: &str,
) -> Result<Option<String>, String> {
    let id = turn.key();
    let resolved = crate::security::resolve_for_containment(Path::new(path))
        .ok_or_else(|| format!("{ERR_WRITE_FAILED}: {path} cannot be written to"))?;
    let path = resolved.as_str();
    crate::commands::file::authorize_source_write(state, path)?;
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let open_elsewhere = store
            .find_active_by_source_path(path)
            .map_err(|e| format!("{ERR_WRITE_FAILED}: {e}"))?
            .is_some_and(|doc| doc.id != id);
        if open_elsewhere {
            return Err(format!("{ERR_WRITE_FAILED}: {path} is open in another tab"));
        }
        store
            .attach_source_path(id, path)
            .map_err(|e| format!("{ERR_WRITE_FAILED}: {e}"))?;
    }
    let target = Path::new(path);
    state.follow_note_path(id, target);
    restore_in_turn(state, turn, content).inspect_err(|_| {
        let store = recover_poison(state.store.lock(), "commands::buffer::save_note_to_inner");
        match store.detach_source_path(id, path) {
            Ok(true) => {
                state.stop_following_note(id);
                state.forget_source_record(id);
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(buffer_id = %id, error = %e, "a chosen path could not be taken back");
            }
        }
    })
}

/// IPC: [`save_note_to_inner`].
#[tauri::command]
pub async fn save_note_to<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    content: String,
    path: String,
) -> Result<Option<String>, String> {
    let ticket = note_ticket(&app, &id);
    let reindex = app.clone();
    run_in_turn(app, ticket, ERR_WRITE_FAILED, move |state, turn| {
        let disk_hash = save_note_to_in_turn(state, turn, &content, &path)?;
        schedule_reindex(&reindex, state, turn.key());
        Ok(disk_hash)
    })
    .await
}

/// Spawns the worker that reindexes a buffer once its edits settle. The worker
/// waits one debounce window, and either reindexes (the buffer stopped
/// changing) or waits again (a newer edit arrived), so an edit burst collapses
/// to a single reindex of the latest on-disk content (ADR-020).
///
/// The worker is a blocking-pool thread, since the reindex reads the file
/// (TS-23).
fn spawn_deferred_reindex<R: Runtime>(app: AppHandle<R>, id: String, first_generation: u64) {
    tauri::async_runtime::spawn_blocking(move || {
        let mut seen = first_generation;
        loop {
            std::thread::sleep(FTS_REINDEX_DEBOUNCE);
            let state = app.state::<AppState>();
            match close_debounce_window(&state, &id, seen) {
                Some(generation) => seen = generation,
                None => break,
            }
        }
    });
}

/// What the deferred worker does once a debounce window has passed: while
/// newer edits keep arriving it waits again, on the generation returned; once
/// they stop it reindexes the buffer ([`reindex_settled_buffer`]) and returns
/// `None`.
pub fn close_debounce_window(state: &AppState, id: &str, seen: u64) -> Option<u64> {
    match state.fts_scheduler.poll(id, seen) {
        PollOutcome::Wait(generation) => Some(generation),
        PollOutcome::Reindex => {
            reindex_settled_buffer(state, id);
            None
        }
        PollOutcome::Taken => None,
    }
}

/// Reindexes now every note whose deferred reindex has not run yet, so a
/// search that follows reads the text as it was last saved rather than as the
/// last debounce window left it (F25). Each file is read in a turn on its
/// note, so a write of it still landing lands first, and with the store
/// released, as the deferred worker reads it.
pub fn reindex_pending_now(state: &AppState) {
    for id in state.fts_scheduler.take_pending() {
        let _turn = state.buffer_writes.turn(&id);
        reindex_settled_buffer(state, &id);
    }
}

/// Reindexes a buffer whose edits have settled: the step the deferred worker
/// runs once its debounce window passes with no newer edit (ADR-020).
///
/// A file the notes index holds is announced once its rows land
/// ([`announce_indexed`]): the watcher drops the save, so this is how the
/// outline, the tags and the graph hear of it. A file anywhere else, or under
/// a name the walk of the notes folder skips, has no rows to change and is not
/// announced.
pub fn reindex_settled_buffer(state: &AppState, id: &str) {
    announce_indexed(&state.event_bus, reindex_buffer_now(state, id));
}

/// Reindexes a buffer's file, and returns the path to announce it under when
/// the notes index holds the file.
///
/// The file is read without the store, which is held only to find the row and
/// to write what was read (TS-23). A row the index takes nothing from has no
/// file read at all (EP-15), and one whose file moved while it was read is
/// left to whatever moved it.
fn reindex_buffer_now(state: &AppState, id: &str) -> Option<String> {
    let target = match state.store.lock() {
        Ok(store) => match store.reindex_target(id) {
            Ok(target) => target,
            Err(e) => {
                tracing::debug!(buffer_id = %id, error = %e, "fts reindex failed");
                None
            }
        },
        Err(_) => {
            tracing::debug!(buffer_id = %id, "store poisoned; skipping reindex");
            None
        }
    }?;
    let content = writ_storage::buffer_store::read_indexable_text(&target);
    let indexed = {
        let store = state.store.lock().ok()?;
        match store.get(id) {
            Ok(now) if now.source_path == target.source_path => {}
            _ => return None,
        }
        match store.reindex_with(&target, &content) {
            Ok(()) => target.source_path,
            Err(e) => {
                tracing::debug!(buffer_id = %id, error = %e, "fts reindex failed");
                None
            }
        }
    };
    let path = indexed?;
    holds_own_write(state, Path::new(&path))
        .then(|| writ_storage::notes_index::index_key(Path::new(&path)))
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
///
/// A file past the size Writ opens is measured and not read
/// ([`BufferStore::read_content`]). The size found goes on the row: the
/// editor states it from there, and the index, which picks files by the row's
/// size, leaves the file alone. The record of what the file held does not
/// move, since nothing was read.
///
/// The file is read without the store, which is held to find the row and again
/// to record what was read (ADR-047); the note's turn keeps a save of it from
/// landing in between. The bytes are hashed once.
pub fn read_buffer_content_inner(state: &AppState, id: &str) -> Result<Vec<u8>, String> {
    let turn = state.buffer_writes.turn(id);
    read_in_turn(state, &turn)
}

/// [`read_buffer_content_inner`] for a caller already holding the note's turn.
pub fn read_in_turn(state: &AppState, turn: &Turn) -> Result<Vec<u8>, String> {
    let id = turn.key();
    let doc = state
        .store
        .lock()
        .map_err(|e| format!("{ERR_READ_FAILED}: {e}"))?
        .get(id)
        .map_err(|e| read_failure_message(&e))?;
    let read = match writ_storage::buffer_store::read_doc_text(&doc) {
        Ok(read) => read,
        Err(error) => {
            if let StorageError::FileTooLarge { size_bytes } = error {
                let store = recover_poison(state.store.lock(), "commands::buffer::read_in_turn");
                if let Err(e) = store.record_size(id, size_bytes) {
                    tracing::debug!(buffer_id = %id, error = %e, "the measured size was not recorded");
                }
            }
            return Err(read_failure_message(&error));
        }
    };
    let content = read.text.as_str();
    if let (Some(source_path), Some(bytes)) = (doc.source_path.as_deref(), read.bytes()) {
        let path = Path::new(source_path);
        let digest = writ_core::hash::sha256_bytes(bytes);
        let size = bytes.len() as u64;
        if doc.read_only {
            state.record_disk_state(id, path, digest, size);
        } else {
            let store = recover_poison(state.store.lock(), "commands::buffer::read_in_turn");
            // What is recorded and kept is the file's bytes, which for a file
            // that is not UTF-8 are not the text handed to the tab: the next
            // save is guarded against them, and the history keeps them as
            // they were (ADR-049).
            //
            // The reload of an externally changed note comes through here, so
            // this is where the tab learns what the file holds, and where the
            // row learns it too (`BufferStore::note_synced_from_disk`).
            match store.note_synced_from_disk(id, digest, size) {
                Ok(Some(disk)) => state.set_disk_state(id, disk),
                Ok(None) => state.record_disk_state(id, path, digest, size),
                Err(e) => {
                    tracing::debug!(buffer_id = %id, error = %e, "the read was not recorded on the row");
                    state.record_disk_state(id, path, digest, size);
                }
            }
            // A file that gained or lost its carriage returns while Writ had
            // it open is followed rather than written back the old way.
            let ending = writ_core::notes::line_ending::LineEnding::detect(content);
            if ending != doc.line_ending {
                if let Err(e) = store.set_line_ending(id, ending) {
                    tracing::debug!(buffer_id = %id, error = %e, "the file's line ending could not be recorded");
                }
            }
            // So is a byte order mark another program added or took away.
            let encoding = doc.encoding.unwrap_or_else(TextEncoding::utf8);
            let found = encoding.with_bom_of(bytes);
            if doc.encoding != Some(found) {
                if let Err(e) = store.set_encoding(id, Some(found)) {
                    tracing::debug!(buffer_id = %id, error = %e, "the file's encoding could not be recorded");
                }
            }
            drop(store);
            // The text the tab is about to be given is kept. The text it is
            // replacing was kept when Writ read or wrote it, which is what
            // puts the pre-overwrite text in the store before the reload
            // lands.
            keep_what_was_seen(state, path, bytes);
        }
    }
    Ok(read.text.into_bytes())
}

/// IPC: [`read_buffer_content_inner`] as the frontend sees it.
///
/// Returns `tauri::ipc::Response` so the browser-side `invoke()` yields an
/// `ArrayBuffer` directly, bypassing JSON string-escaping. The caller
/// decodes with `new TextDecoder().decode(bytes)`.
#[tauri::command]
pub async fn read_buffer_content<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<tauri::ipc::Response, String> {
    let ticket = note_ticket(&app, &id);
    run_in_turn(app, ticket, ERR_READ_FAILED, |state, turn| {
        read_in_turn(state, turn).map(tauri::ipc::Response::new)
    })
    .await
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
///
/// Answers with the bytes it read as well, `None` when nothing was read, so the
/// caller that keeps them does not read the file again.
fn note_file_answer_of(
    path: &Path,
    st_flags: Option<u32>,
    encoding: TextEncoding,
) -> Result<(NoteDiskAnswer, Option<Vec<u8>>), String> {
    if is_not_downloaded(st_flags) {
        return Ok((NoteDiskAnswer::Undescribed, None));
    }
    Ok(
        match writ_storage::buffer_store::read_note_file(path, encoding)
            .map_err(|e| e.to_string())?
        {
            Some((state, bytes)) => (
                NoteDiskAnswer::Described {
                    disk: DiskStateDto::from(state),
                },
                Some(bytes),
            ),
            None => (NoteDiskAnswer::Undescribed, None),
        },
    )
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
pub async fn note_disk_state<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<NoteDiskAnswer, String> {
    let ticket = note_ticket(&app, &id);
    run_in_turn(app, ticket, ERR_READ_FAILED, note_disk_state_in_turn).await
}

/// [`note_disk_state`] as everything but the IPC boundary sees it.
pub fn note_disk_state_inner(state: &AppState, id: &str) -> Result<NoteDiskAnswer, String> {
    let turn = state.buffer_writes.turn(id);
    note_disk_state_in_turn(state, &turn)
}

/// [`note_disk_state_inner`] for a caller already holding the note's turn.
///
/// One read answers the editor and keeps the version: the bytes the state is
/// computed from are the bytes kept.
fn note_disk_state_in_turn(state: &AppState, turn: &Turn) -> Result<NoteDiskAnswer, String> {
    let id = turn.key();
    let (source_path, read_only, encoding) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let doc = store.get(id).map_err(|e| e.to_string())?;
        let Some(source_path) = doc.source_path else {
            return Ok(NoteDiskAnswer::NoFile);
        };
        (
            source_path,
            doc.read_only,
            doc.encoding.unwrap_or_else(TextEncoding::utf8),
        )
    };
    let path = Path::new(&source_path);
    let (answer, bytes) = note_file_answer_of(
        path,
        writ_storage::buffer_store::dataless_flags(path),
        encoding,
    )?;
    // This is what a tab asks after it is told its file changed, and the
    // answer decides whether it reloads at all. A tab that leaves the
    // question standing still leaves the text on disk in the store, so a
    // second change over the top of the first does not take the first with
    // it. Nothing is kept for a file the store would refuse anyway.
    if let (NoteDiskAnswer::Described { disk }, Some(bytes)) = (&answer, bytes) {
        if !read_only && writ_core::note_history::is_versionable(disk.size) {
            keep_what_was_seen(state, path, &bytes);
        }
    }
    Ok(answer)
}

/// What resolving a change outside Writ left behind.
///
/// `conflict_copy_path` names the file the losing text was written to. It is
/// `None` only when the two texts turned out to be the same text, which the
/// dirty predicate allows: it fails closed, so a note whose document has not
/// been compared yet asks the question anyway (ADR-033 §6).
///
/// `content` is what the tab must show afterwards, and is `None` when the tab
/// keeps what it is already holding. `disk_hash` is the comparison digest of
/// what the note's file holds once the choice has run, so the tab stops
/// reading dirty without going back to disk for the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolveOutcome {
    pub conflict_copy_path: Option<String>,
    pub content: Option<String>,
    pub disk_hash: String,
}

/// Carries out the choice a person made about a file that changed under them.
///
/// The text the choice does not keep is written beside the note first, and the
/// note is only touched once that copy is on disk
/// ([`writ_core::notes::reload::apply_choice`]). A copy that cannot be written
/// stops the whole thing, because the alternative is a write that lands over
/// a text nothing else holds.
///
/// The editor's text arrives as an argument. It has to: the file is the only
/// copy of a note (ADR-028 §1), so the one place the unsaved version exists is
/// the document, and Rust reading the note back would read the very change
/// being asked about.
///
/// What the answer wrote is indexed before it returns and announced as one
/// event (ADR-036 section 4): the copy, the note when Keep mine rewrote it,
/// and the copy Keep mine's save set aside when the guard stopped it. The
/// watcher reports none of them, and a bar is answered once, so the deferral
/// that collapses a run of keystroke saves has nothing to collapse.
///
/// An answer is a write of the tab's text, so it reaches the unsaved-text
/// journal like a save does ([`keep_unsaved_text`]). Every answer that returns
/// leaves that text in a file, the note's own or the copy beside it, and
/// clears the tab's entry. One that fails keeps the text, unless its write was
/// stopped by the guard, which left the text in a copy beside the note.
pub fn resolve_external_change_inner(
    state: &AppState,
    id: &str,
    choice: ChangeChoice,
    content: &str,
) -> Result<ResolveOutcome, String> {
    let turn = state.buffer_writes.turn(id);
    resolve_in_turn(state, &turn, choice, content)
}

/// [`resolve_external_change_inner`] for a caller already holding the note's
/// turn.
fn resolve_in_turn(
    state: &AppState,
    turn: &Turn,
    choice: ChangeChoice,
    content: &str,
) -> Result<ResolveOutcome, String> {
    let id = turn.key();
    let answered = answer_external_change(state, turn, choice, content);
    let landing = match &answered {
        Ok(_) => SaveLanding::Landed,
        Err(refused) => refused.landing,
    };
    keep_unsaved_text(state, id, content, landing, None);
    answered.map_err(|refused| refused.message)
}

/// [`resolve_external_change_inner`]'s answer, saying what became of the
/// tab's text when there was none.
fn answer_external_change(
    state: &AppState,
    turn: &Turn,
    choice: ChangeChoice,
    content: &str,
) -> Result<ResolveOutcome, SaveRefused> {
    let id = turn.key();
    // An answer is a write, so it waits out a held removal like any other one
    // and for the same reasons ([`wait_out_a_held_removal`]): the path read
    // below is the one a move has already left the row on, and a file that
    // turned out to be gone is refused rather than written back. Before the
    // store lock, which answering a hold needs.
    wait_out_a_held_removal(state, id, RemovedFile::Refuse).map_err(SaveRefused::not_landed)?;
    let (source_path, line_ending, encoding) = {
        let store = state
            .store
            .lock()
            .map_err(|e| SaveRefused::not_landed(e.to_string()))?;
        let doc = store
            .get(id)
            .map_err(|e| SaveRefused::not_landed(e.to_string()))?;
        if doc.read_only {
            return Err(SaveRefused {
                landing: SaveLanding::ReadOnlyNote,
                message: format!("{ERR_NOTE_READ_ONLY}: file {id} is read-only"),
                minted: None,
            });
        }
        let ending = doc.line_ending;
        let encoding = doc.encoding.unwrap_or_else(TextEncoding::utf8);
        let path = doc.source_path.ok_or_else(|| {
            SaveRefused::not_landed(format!("{ERR_FILE_MISSING}: file {id} is on disk nowhere"))
        })?;
        (path, ending, encoding)
    };
    let path = Path::new(&source_path);
    answer_external_change_at(
        state,
        turn,
        path,
        dataless_flags(path),
        line_ending,
        encoding,
        choice,
        content,
    )
}

/// [`resolve_external_change_inner`] once the file's path and flags are known.
///
/// The flags are a parameter for the same reason [`note_file_answer_of`]'s are:
/// a file whose bytes are not on this machine must be refused without being
/// read, and that refusal is only testable where the flags can be supplied.
/// `line_ending` and `encoding` are the note's, read off the same row as the
/// path.
#[allow(clippy::too_many_arguments)]
pub fn resolve_external_change_at(
    state: &AppState,
    id: &str,
    path: &Path,
    st_flags: Option<u32>,
    line_ending: LineEnding,
    encoding: TextEncoding,
    choice: ChangeChoice,
    content: &str,
) -> Result<ResolveOutcome, String> {
    let turn = state.buffer_writes.turn(id);
    answer_external_change_at(
        state,
        &turn,
        path,
        st_flags,
        line_ending,
        encoding,
        choice,
        content,
    )
    .map_err(|refused| refused.message)
}

/// [`resolve_external_change_at`]'s answer, saying what became of the tab's
/// text when there was none.
///
/// `Keep mine` writes through [`write_note_source_unannounced`], and that
/// write's refusal comes back as it is: a write the guard stopped left the
/// text in a copy beside the note, and journaling it again would put a second
/// copy there at the next launch.
#[allow(clippy::too_many_arguments)]
fn answer_external_change_at(
    state: &AppState,
    turn: &Turn,
    path: &Path,
    st_flags: Option<u32>,
    line_ending: LineEnding,
    encoding: TextEncoding,
    choice: ChangeChoice,
    content: &str,
) -> Result<ResolveOutcome, SaveRefused> {
    let id = turn.key();
    if is_not_downloaded(st_flags) {
        return Err(SaveRefused::not_landed(format!(
            "{ERR_FILE_NOT_DOWNLOADED}: {} has not been downloaded",
            path.display()
        )));
    }
    crate::commands::file::authorize_source_write(state, &path.to_string_lossy())
        .map_err(SaveRefused::not_landed)?;

    // One read. Two would let the copy and the text handed to the tab come
    // from different versions of the file, which is the failure this whole
    // unit is about.
    let disk_bytes = std::fs::read(path).map_err(|e| {
        SaveRefused::not_landed(format!(
            "{}: {} could not be read ({e})",
            io_failure_code(e.kind()),
            path.display()
        ))
    })?;
    // Read in the note's encoding (ADR-049), as the tab would read it.
    let disk_text = encoding
        .decode(&disk_bytes)
        .map_err(|_| {
            SaveRefused::not_landed(format!(
                "{ERR_WRITE_FAILED}: {} is not text",
                path.display()
            ))
        })?
        .into_owned();

    let disk_hash = encoding.comparison_digest_hex(&disk_bytes);
    let mine_hash = writ_core::hash::comparison_digest_hex(content.as_bytes());

    // The two texts are the same text. Writing a copy would put a duplicate
    // in the notes folder for a difference nobody can see, and writing the
    // note would rewrite its line endings for one.
    if disk_hash == mine_hash {
        state.record_disk_state_bytes(id, path, &disk_bytes);
        return Ok(ResolveOutcome {
            conflict_copy_path: None,
            content: None,
            disk_hash,
        });
    }

    // Both sides, before either is applied (ADR-028 §5, spec 205). The file's
    // text is about to be replaced or the tab's is about to be dropped, and
    // which of the two it is depends on an answer that has not run yet.
    keep_what_was_seen(state, path, &disk_bytes);
    let mine = line_ending.apply(content);
    match encoding.encode(&mine) {
        Ok(bytes) => keep_what_was_seen(state, path, &bytes),
        Err(_) => keep_what_was_seen(state, path, mine.as_bytes()),
    }

    let outcome = apply_choice(choice);
    let losing: Cow<'_, str> = match outcome.write_conflict_copy_of {
        // The editor's text is LF whatever the file uses, so the note's own
        // ending goes back on: the copy is the note's text in a file of its
        // own, and a save of the note would have written it the same way.
        Side::Mine => line_ending.apply(content),
        // The bytes the file was found holding, kept as they were found. The
        // recorded ending is the note's, and the program that just rewrote
        // this file is under no obligation to have used it.
        Side::Disk => Cow::Borrowed(disk_text.as_str()),
    };
    let stamp = ignore_stamper(state);
    let copy = write_conflict_copy(path, &losing, chrono::Utc::now(), Some(&stamp))
        .map_err(|e| SaveRefused::not_landed(save_failure_message(&e)))?;
    let conflict_copy_path = Some(copy.to_string_lossy().into_owned());
    let copy_key = index_copy(state, &copy);
    if outcome.write_conflict_copy_of == Side::Mine {
        recover_poison(
            state.store.lock(),
            "commands::buffer::resolve_external_change_at",
        )
        .remember_text_written_beside(id, content);
    }

    let mut rewritten = None;
    let mut set_aside = Vec::new();
    let answered = match outcome.then {
        Action::WriteMine => {
            // What the file holds is beside the note now, so the write guard
            // has nothing left to protect and the save may land. Recording it
            // is also what makes the record true: Writ has read this file.
            state.record_disk_state_bytes(id, path, &disk_bytes);
            write_note_source_unannounced(state, turn, content, RemovedFile::Refuse, &mut set_aside)
                .map(|written| {
                    rewritten = reindex_buffer_now(state, id);
                    ResolveOutcome {
                        conflict_copy_path,
                        content: None,
                        disk_hash: written.unwrap_or(mine_hash),
                    }
                })
        }
        Action::TakeDisk | Action::TakeDiskAndShowCopy => {
            // The file is left exactly as the other program wrote it. The tab
            // takes its text, and the record moves with it.
            state.record_disk_state_bytes(id, path, &disk_bytes);
            Ok(ResolveOutcome {
                conflict_copy_path,
                content: Some(disk_text),
                disk_hash,
            })
        }
    };
    announce_indexed(
        &state.event_bus,
        rewritten.into_iter().chain(copy_key).chain(set_aside),
    );
    answered
}

/// IPC: [`resolve_external_change_inner`] as the bar calls it.
#[tauri::command]
pub async fn resolve_external_change<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    choice: ChangeChoice,
    content: String,
) -> Result<ResolveOutcome, String> {
    let ticket = note_ticket(&app, &buffer_id);
    run_in_turn(app, ticket, ERR_WRITE_FAILED, move |state, turn| {
        resolve_in_turn(state, turn, choice, &content)
    })
    .await
}

/// One note's text as the editor holds it, for a save that could not land.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UnsavedNote {
    pub id: String,
    pub content: String,
}

/// IPC: keeps text the editor holds that no save could write in the
/// unsaved-text journal (ADR-044; spec S2, the quit-with-a-failed-save case).
///
/// Called on the way out: when a tab closes, when the window closes and when
/// Writ quits, with whatever the editor holds that is not known to be on disk.
/// Called too while a bar holds a note's saves, on the autosave's debounce,
/// with the text the editor is holding back. A save that failed has journaled
/// its text already. What this adds is text the editor held back while a bar
/// was asking about the note's file, which no save ever carried. The next
/// launch restores all of it ([`crate::state::restore_unsaved_text`]).
///
/// Text a dated copy beside the note already holds is left out, compared by
/// hash: a save the guard stopped wrote it there, and journaling it would put
/// a second copy beside the note at the next launch (ADR-033 decision 15).
#[tauri::command]
pub fn record_unsaved_notes(
    state: State<'_, AppState>,
    notes: Vec<UnsavedNote>,
) -> Result<(), String> {
    record_unsaved_notes_inner(&state, notes)
}

/// [`record_unsaved_notes`] as everything but the IPC boundary sees it.
///
/// Every note is tried. A note whose text could not be kept is logged, and
/// the call fails once the rest are written, so the editor keeps its own copy
/// of that text rather than drop it.
pub fn record_unsaved_notes_inner(state: &AppState, notes: Vec<UnsavedNote>) -> Result<(), String> {
    let store = recover_poison(state.store.lock(), "commands::buffer::record_unsaved_notes");
    let now = chrono::Utc::now();
    let total = notes.len();
    let mut unkept = 0usize;
    for note in notes {
        if store.copy_beside_holds(&note.id, &note.content) {
            continue;
        }
        if let Err(e) = store.record_unsaved_text(&note.id, &note.content, now) {
            tracing::warn!(buffer_id = %note.id, error = %e, "unsaved text could not be journaled");
            unkept += 1;
        }
    }
    if unkept > 0 {
        return Err(format!(
            "{ERR_WRITE_FAILED}: the unsaved text of {unkept} of {total} files could not be kept"
        ));
    }
    Ok(())
}

/// IPC: drops the unsaved-text journal entries of the tabs whose unsaved text
/// the person chose to lose ("Close and lose changes").
///
/// The editor drops its own copy at the same moment. Without this, the text
/// the save that failed left in the journal would come back at the next
/// launch, beside the note, after the person was told it was gone. Text an
/// earlier tab of the note handed over was never in the closing tab, so it is
/// not what the person chose to lose, and it stays (ADR-044 §3).
#[tauri::command]
pub fn discard_unsaved_text(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    discard_unsaved_text_inner(&state, &ids)
}

/// [`discard_unsaved_text`] as everything but the IPC boundary sees it.
pub fn discard_unsaved_text_inner(state: &AppState, ids: &[String]) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    for id in ids {
        store.clear_unsaved_text(id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// IPC: [`list_active_buffers_inner`], off the main thread: it measures every
/// restored file and starts following each one.
#[tauri::command]
pub async fn list_active_buffers<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<BufferDocument>, String> {
    run_blocking(app, ERR_READ_FAILED, list_active_buffers_inner).await
}

/// The tabs the last session left open, each file measured before anything
/// reads it.
///
/// A file can change size while Writ is closed, and the row is where the
/// editor and the index read its tier from. Each file is measured with a
/// stat, outside the store's lock and never with a read, and a size that
/// moved is recorded on the row without the stamp a read leaves.
/// A path with no file behind it is listed at no size, so nothing waits on the
/// size it had and its read says at once what became of it. Its row keeps the
/// size on record: a file that comes back large is still one the reads that
/// pick files by the row's size leave alone.
///
/// This is also where a restored tab starts being followed. A tab nobody has
/// brought to the front is the one most likely to be sitting on a file that
/// moved on, so waiting for the user to click it would leave the whole
/// restored session unwatched.
pub fn list_active_buffers_inner(state: &AppState) -> Result<Vec<BufferDocument>, String> {
    let mut docs = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .list_by_status(BufferStatus::Active)
            .map_err(|e| e.to_string())?
    };
    let mut moved: Vec<(String, u64)> = Vec::new();
    for doc in docs.iter_mut().filter(|doc| doc.source_path.is_some()) {
        match measured_size(doc) {
            Some(size) if size != doc.size_bytes => {
                doc.size_bytes = size;
                moved.push((doc.id.clone(), size));
            }
            Some(_) => {}
            None => doc.size_bytes = 0,
        }
    }
    if !moved.is_empty() {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        for (id, size) in moved {
            if let Err(e) = store.record_size(&id, size) {
                tracing::debug!(buffer_id = %id, error = %e, "the measured size was not recorded");
            }
        }
    }
    for doc in &docs {
        state.follow_note_file(doc);
    }
    Ok(docs)
}

/// The size of the file behind `doc`, from a stat: `None` for a note with no
/// file, and for a path that holds no file.
pub(crate) fn measured_size(doc: &BufferDocument) -> Option<u64> {
    let meta = std::fs::metadata(doc.source_path.as_deref()?).ok()?;
    meta.is_file().then_some(meta.len())
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

/// The file a row that just stopped being an open tab named, when no other
/// open tab shows it: the one whose write permission goes with the tab.
///
/// Asked under the store lock, after the row has moved; the blessing itself is
/// taken away once the lock is let go, since resolving the path asks the disk.
pub(crate) fn released_source(store: &BufferStore, source_path: Option<String>) -> Option<String> {
    let path = source_path?;
    match store.find_active_by_source_path(&path) {
        Ok(None) => Some(path),
        Ok(Some(_)) => None,
        Err(error) => {
            tracing::warn!(%error, "could not tell whether another tab shows a closed file");
            Some(path)
        }
    }
}

/// Takes away the write permission of every file in `released`.
///
/// A closed tab's file is one the person has put away, and only an open tab
/// may write a file from outside the notes folder; reopening it blesses it
/// again.
pub(crate) fn revoke_released(state: &AppState, released: &[String]) {
    for path in released {
        state.authorized_paths.revoke_blessed_source(path);
    }
}

/// IPC: closes one tab.
///
/// What the file held is forgotten with the tab. The record exists to answer
/// "did this change since Writ last looked", and after a close Writ has not
/// looked; keeping it would let a note edited elsewhere while its tab was
/// closed reopen without a word. So is the permission to write the file.
///
/// Waits for the note's turn, so a write still landing finishes before the
/// record of it is forgotten.
pub fn close_buffer_inner(state: &AppState, id: &str) -> Result<(), String> {
    let _turn = state.buffer_writes.turn(id);
    let released = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let doc = store.get(id).map_err(|e| e.to_string())?;
        forget_ignore_stamp(state, &doc, "commands::buffer::close_buffer");
        // A note whose tab has been closed is one the person has already put
        // away, so its first line no longer renames its file unasked.
        if let Some(path) = doc.source_path.as_deref() {
            state.retitle_watch.closed(std::path::Path::new(path));
        }
        store.close(id).map_err(|e| e.to_string())?;
        released_source(&store, doc.source_path)
    };
    revoke_released(state, released.as_slice());
    state.forget_disk_state(id);
    state.forget_source_record(id);
    state.stop_following_note(id);
    Ok(())
}

#[tauri::command]
pub async fn close_buffer<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        close_buffer_inner(state, &id)
    })
    .await
}

/// [`close_buffer_inner`] for several tabs at once, each closed after its
/// note's turn.
pub fn close_buffers_inner(state: &AppState, ids: &[String]) -> Result<(), String> {
    // One turn per note, taken in one order, so two closes of overlapping
    // sets cannot each hold a turn the other is waiting for.
    let mut order: Vec<&String> = ids.iter().collect();
    order.sort();
    order.dedup();
    let _turns: Vec<Turn> = order
        .into_iter()
        .map(|id| state.buffer_writes.turn(id))
        .collect();
    let released: Vec<String> = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let mut sources = Vec::new();
        for id in ids {
            if let Ok(doc) = store.get(id) {
                if let Some(path) = doc.source_path.as_deref() {
                    state.retitle_watch.closed(std::path::Path::new(path));
                }
                sources.push(doc.source_path);
            }
        }
        store.close_many(ids).map_err(|e| e.to_string())?;
        sources
            .into_iter()
            .filter_map(|source| released_source(&store, source))
            .collect()
    };
    revoke_released(state, &released);
    for id in ids {
        state.forget_disk_state(id);
        state.forget_source_record(id);
        state.stop_following_note(id);
    }
    Ok(())
}

#[tauri::command]
pub async fn close_buffers<R: Runtime>(app: AppHandle<R>, ids: Vec<String>) -> Result<(), String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        close_buffers_inner(state, &ids)
    })
    .await
}

pub fn delete_buffer_inner(state: &AppState, id: &str) -> Result<(), String> {
    let turn = state.buffer_writes.turn(id);
    delete_buffer_in_turn(state, &turn)
}

/// [`delete_buffer_inner`] for a caller already holding the note's turn.
pub(crate) fn delete_buffer_in_turn(state: &AppState, turn: &Turn) -> Result<(), String> {
    let id = turn.key();
    let released = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let doc = store.get(id).ok();
        if let Some(doc) = doc.as_ref() {
            forget_ignore_stamp(state, doc, "commands::buffer::delete_buffer");
        }
        store.delete(id).map_err(|e| e.to_string())?;
        released_source(&store, doc.and_then(|doc| doc.source_path))
    };
    revoke_released(state, released.as_slice());
    state.forget_disk_state(id);
    state.forget_source_record(id);
    state.stop_following_note(id);
    Ok(())
}

#[tauri::command]
pub async fn delete_buffer<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        delete_buffer_inner(state, &id)
    })
    .await
}

#[tauri::command]
pub fn rename_buffer(state: State<'_, AppState>, id: String, title: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.rename(&id, &title).map_err(|e| e.to_string())
}

/// Records the language chosen for a buffer with Change Language, or clears
/// the choice with `None`, and answers the buffer as it now stands.
pub fn set_buffer_language_inner(
    state: &AppState,
    id: &str,
    language: Option<&str>,
) -> Result<BufferDocument, String> {
    writ_core::buffer::language::validate_language_choice(language).map_err(|e| e.to_string())?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .update_language(id, language)
        .map_err(|e| e.to_string())?;
    store.get(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_buffer_language(
    state: State<'_, AppState>,
    id: String,
    language: Option<String>,
) -> Result<BufferDocument, String> {
    set_buffer_language_inner(&state, &id, language.as_deref())
}

/// Code a reopen carries when the encoding asked for is not one Writ reads.
pub const ERR_UNKNOWN_ENCODING: &str = "ERR_UNKNOWN_ENCODING";

/// Reads note `id`'s file in `encoding` and keeps that encoding for it: Reopen
/// with Encoding (ADR-049).
///
/// Only the row changes here. The tab then reloads through
/// [`read_buffer_content`], which reads the file in the encoding just
/// recorded, so the text, the record of what the file holds and the version
/// kept all come from that one read path. The byte order mark and the line
/// ending are taken from the bytes, as an open takes them.
///
/// The file is read before anything is recorded and outside the store lock.
/// An encoding the file is not text in is refused with the read code a tab
/// gets for that ([`read_failure_code`]), and the note keeps the encoding it
/// had.
pub fn reopen_with_encoding_inner(
    state: &AppState,
    id: &str,
    encoding: &str,
) -> Result<BufferDocument, String> {
    let _turn = state.buffer_writes.turn(id);
    let chosen = TextEncoding::from_label(encoding)
        .ok_or_else(|| format!("{ERR_UNKNOWN_ENCODING}: {encoding:?} is not an encoding"))?;
    let source_path = {
        let store = state
            .store
            .lock()
            .map_err(|e| format!("{ERR_READ_FAILED}: {e}"))?;
        let doc = store.get(id).map_err(|e| read_failure_message(&e))?;
        if doc.read_only {
            return Err(format!("{ERR_NOTE_READ_ONLY}: file {id} is read-only"));
        }
        doc.source_path
            .ok_or_else(|| format!("{ERR_READ_FILE_MISSING}: file {id} is on disk nowhere"))?
    };
    let size_bytes = std::fs::metadata(&source_path)
        .map_err(|e| format!("{}: {e}", io_read_failure_code(e.kind())))?
        .len();
    if let writ_core::file_ops::FileOpenMode::Refused { .. } =
        writ_core::file_ops::classify_size(size_bytes, false)
    {
        return Err(read_failure_message(&StorageError::FileTooLarge {
            size_bytes,
        }));
    }
    let bytes = std::fs::read(&source_path)
        .map_err(|e| format!("{}: {e}", io_read_failure_code(e.kind())))?;
    let chosen = chosen.with_bom_of(&bytes);
    let text = chosen
        .decode(&bytes)
        .map_err(|e| read_failure_message(&StorageError::from(e)))?;
    let ending = LineEnding::detect(&text);

    let store = state
        .store
        .lock()
        .map_err(|e| format!("{ERR_READ_FAILED}: {e}"))?;
    store
        .set_encoding(id, Some(chosen))
        .map_err(|e| read_failure_message(&e))?;
    store
        .set_line_ending(id, ending)
        .map_err(|e| read_failure_message(&e))?;
    store.get(id).map_err(|e| read_failure_message(&e))
}

/// IPC: [`reopen_with_encoding_inner`].
#[tauri::command]
pub async fn reopen_with_encoding<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    encoding: String,
) -> Result<BufferDocument, String> {
    run_blocking(app, ERR_READ_FAILED, move |state| {
        reopen_with_encoding_inner(state, &buffer_id, &encoding)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use writ_core::notes::guard::{DiskState, SF_DATALESS};

    fn io(kind: std::io::ErrorKind) -> StorageError {
        StorageError::Io(std::io::Error::new(kind, "from the filesystem"))
    }

    #[test]
    fn every_read_failure_carries_a_code() {
        use std::io::ErrorKind;
        let cases = [
            (io(ErrorKind::InvalidData), ERR_READ_NOT_UTF8),
            (io(ErrorKind::PermissionDenied), ERR_READ_PERMISSION_DENIED),
            (io(ErrorKind::ResourceBusy), ERR_READ_FILE_IN_USE),
            (io(ErrorKind::NotFound), ERR_READ_FILE_MISSING),
            (io(ErrorKind::TimedOut), ERR_READ_TIMED_OUT),
            (io(ErrorKind::Interrupted), ERR_READ_FAILED),
            (
                StorageError::FileTooLarge {
                    size_bytes: 600_000_000,
                },
                writ_core::file_ops::ERR_OPEN_TOO_LARGE,
            ),
            (
                StorageError::Consistency {
                    message: "row and disk disagree".to_string(),
                },
                ERR_READ_FAILED,
            ),
        ];
        for (error, code) in cases {
            assert_eq!(read_failure_code(&error), code, "{error}");
            let message = read_failure_message(&error);
            assert!(
                message.starts_with(&format!("{code}: ")),
                "the code is not in front of the message: {message}"
            );
        }
    }

    #[test]
    fn the_utf8_failure_of_read_to_string_is_the_not_utf8_code() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("latin1.txt");
        std::fs::write(&file, b"caf\xe9").unwrap();
        let error = StorageError::from(std::fs::read_to_string(&file).unwrap_err());

        assert_eq!(read_failure_code(&error), ERR_READ_NOT_UTF8);
    }

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
            note_file_answer_of(&missing, None, TextEncoding::utf8())
                .unwrap()
                .0,
            NoteDiskAnswer::Undescribed
        );
    }

    #[test]
    fn a_normal_file_reports_the_digest_and_the_length_of_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, b"one line\n").unwrap();

        let state = described(
            note_file_answer_of(&path, None, TextEncoding::utf8())
                .unwrap()
                .0,
        );
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
            note_file_answer_of(&path, Some(SF_DATALESS), TextEncoding::utf8())
                .unwrap()
                .0,
            NoteDiskAnswer::Undescribed
        );
    }

    #[test]
    fn a_crlf_file_reports_the_digest_its_writes_report() {
        // CodeMirror converts the line endings on load, so every write of the
        // document is the LF form and reports the LF digest. Reporting the raw
        // digest here would make every CRLF file read as changed on disk for
        // ever.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("windows.md");
        std::fs::write(&path, b"one\r\ntwo\r\n").unwrap();

        let state = described(
            note_file_answer_of(&path, None, TextEncoding::utf8())
                .unwrap()
                .0,
        );
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
    fn the_resolve_outcome_reaches_the_editor_with_the_names_it_reads() {
        let resolved = serde_json::to_value(ResolveOutcome {
            conflict_copy_path: Some("/notes/a (conflict 2026-09-05 10.00.00).md".to_string()),
            content: Some("what the file holds".to_string()),
            disk_hash: "abc".to_string(),
        })
        .unwrap();
        assert_eq!(
            resolved["conflict_copy_path"],
            "/notes/a (conflict 2026-09-05 10.00.00).md"
        );
        assert_eq!(resolved["content"], "what the file holds");
        assert_eq!(resolved["disk_hash"], "abc");

        // A tab that keeps its own text is told so by a null rather than by a
        // field that is not there: the editor branches on the value.
        let kept = serde_json::to_value(ResolveOutcome {
            conflict_copy_path: None,
            content: None,
            disk_hash: "abc".to_string(),
        })
        .unwrap();
        assert!(kept["content"].is_null());
        assert!(kept["conflict_copy_path"].is_null());
        assert_eq!(kept["disk_hash"], "abc");
    }

    #[test]
    fn the_three_choices_deserialise_from_the_words_the_bar_sends() {
        for (word, expected) in [
            ("keep_mine", ChangeChoice::KeepMine),
            ("use_disk", ChangeChoice::UseDisk),
            ("keep_both", ChangeChoice::KeepBoth),
        ] {
            assert_eq!(
                serde_json::from_value::<ChangeChoice>(serde_json::Value::String(word.to_string()))
                    .unwrap(),
                expected
            );
        }
        assert!(
            serde_json::from_value::<ChangeChoice>(serde_json::Value::String(
                "keepMine".to_string()
            ))
            .is_err()
        );
    }

    #[test]
    fn a_time_the_filesystem_could_not_report_is_carried_as_none() {
        let dto = DiskStateDto::from(file_state(writ_core::hash::sha256_bytes(b""), 0, None));
        assert_eq!(dto.mtime_ms, None);
    }
}
