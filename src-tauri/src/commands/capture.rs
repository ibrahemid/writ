//! IPC for the quick capture note (ADR-055): the entry added to Today's File,
//! the file it goes to, and closing the note.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tracing::{info, warn};
use writ_core::capture::{append_entry, capture_entry};
use writ_core::notes::guard::{is_not_downloaded, DiskState};
use writ_core::notes::{name_is_taken, WriteOrigin};
use writ_core::text_encoding::{decode_file, TextEncoding};
use writ_storage::buffer_store::dataless_flags;
use writ_storage::errors::StorageError;
use writ_storage::guarded::{
    create_note_guarded, keep_versions, write_note_guarded, ConflictPolicy, CreateNote, DiskRead,
    GuardedWrite, TakenName, WriteCapture,
};
use writ_storage::notes_index;
use writ_storage::paths::file_name_only;
use writ_storage::write_lock::WriteLock;

use crate::commands::buffer::{ignore_stamper, save_failure_message, ERR_WRITE_FAILED};
use crate::commands::note_index::{announce_indexed, holds_own_write};
use crate::commands::notes::{find_todays_file, path_is_inside_notes, TodaysFileAt};
use crate::commands::offload::run_blocking;
use crate::state::AppState;
use crate::watcher::open_files::OpenNotes;

/// How many times an entry reads Today's File again when the file changed
/// between the read and the rename.
pub const CAPTURE_ATTEMPTS: usize = 3;

/// What adding an entry came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOutcome {
    /// Today's File, by name.
    pub name: String,
    /// Whether the note held anything to add. A blank note writes nothing.
    pub added: bool,
}

/// One try at the append, and whether another is worth making.
enum Attempt {
    /// The file moved on under this try; read it again.
    Moved(StorageError),
    /// Nothing another read would change.
    Stopped(String),
}

/// The sentence for a Today's File that resolves outside the notes folder.
fn outside_message(name: &str) -> String {
    format!("{name} is outside your folder, so nothing was written.")
}

/// The sentence for a Today's File whose bytes are not text.
fn not_text_message(name: &str) -> String {
    format!("{name} is not a text file, so nothing was written.")
}

/// Adds what was typed into the capture note to the end of Today's File,
/// minting the file when the day has none.
///
/// # Errors
///
/// A failure the editor's save would answer with the same code
/// (`save_failure_message`), or a sentence for a file that is not text, sits
/// outside the notes folder, or wears the dated name as a folder.
pub fn capture_append_inner(
    state: &AppState,
    now: DateTime<Utc>,
    typed: &str,
) -> Result<CaptureOutcome, String> {
    let Some(entry) = capture_entry(typed) else {
        return Ok(CaptureOutcome {
            name: capture_target_inner(state, now)?,
            added: false,
        });
    };
    let mut moved = None;
    for _ in 0..CAPTURE_ATTEMPTS {
        let attempt = match find_todays_file(state, now)? {
            TodaysFileAt::Existing(path) => append_to(state, &path, &entry),
            TodaysFileAt::Missing(path) => mint_with(state, now, &path, &entry),
        };
        match attempt {
            Ok(outcome) => {
                info!(note = %outcome.name, "quick capture added an entry");
                return Ok(outcome);
            }
            Err(Attempt::Moved(error)) => moved = Some(error),
            Err(Attempt::Stopped(message)) => return Err(message),
        }
    }
    Err(match moved {
        Some(StorageError::NoteNameTaken { name, .. }) => name_is_taken(&name),
        Some(error) => save_failure_message(&error),
        None => ERR_WRITE_FAILED.to_string(),
    })
}

/// The name of the file an entry goes to now.
///
/// # Errors
///
/// The dated name is worn by a folder or a link to one.
pub fn capture_target_inner(state: &AppState, now: DateTime<Utc>) -> Result<String, String> {
    let path = match find_todays_file(state, now)? {
        TodaysFileAt::Existing(path) | TodaysFileAt::Missing(path) => path,
    };
    Ok(file_name_only(&path.to_string_lossy()))
}

/// Adds `entry` to the end of the file at `path`.
///
/// A tab holding the file is waited for and held off for the length of the
/// write ([`AppState::buffer_writes`]), so a save of it lands before the read
/// or after the tab is told. The tab is told the way the watcher tells it, and
/// its record of the file is left alone: a tab with unsaved text that still
/// believes the old bytes is then turned down by the guard rather than
/// writing over the entry.
fn append_to(state: &AppState, path: &Path, entry: &str) -> Result<CaptureOutcome, Attempt> {
    let name = file_name_only(&path.to_string_lossy());
    if !path_is_inside_notes(state, &path.to_string_lossy()) {
        return Err(Attempt::Stopped(outside_message(&name)));
    }
    let resolved = crate::security::resolve_for_containment(path);
    let tab = resolved
        .as_deref()
        .and_then(|resolved| state.open_tabs().note_at(Path::new(resolved)));
    let _turn = tab.as_deref().map(|id| state.buffer_writes.turn(id));

    // Before the read, which is what would pull an evicted file down.
    if is_not_downloaded(dataless_flags(path)) {
        return Err(Attempt::Stopped(save_failure_message(
            &StorageError::SourceNotDownloaded {
                path: path.to_string_lossy().into_owned(),
            },
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| Attempt::Stopped(save_failure_message(&StorageError::Io(error))))?;
    let Some((encoding, text)) = decode_for_append(state, tab.as_deref(), &bytes) else {
        return Err(Attempt::Stopped(not_text_message(&name)));
    };
    let appended = append_entry(&text, entry);
    let written = encoding
        .encode(&appended)
        .map_err(|lossy| Attempt::Stopped(save_failure_message(&StorageError::from(lossy))))?
        .into_owned();

    // The record the write is judged against is the read it was built from:
    // the entry goes after exactly those bytes or not at all.
    let read = DiskState {
        hash: writ_core::hash::sha256_bytes(&bytes),
        size: bytes.len() as u64,
        mtime: std::fs::metadata(path).ok().and_then(|m| m.modified().ok()),
    };
    let keep = keep_versions(&state.note_history);
    let lock = WriteLock::in_data_folder(&state.writ_dir);
    let stamp = ignore_stamper(state);
    let outcome = write_note_guarded(
        GuardedWrite {
            target: path,
            bytes: &written,
            last_known: Some(read),
            on_disk: DiskRead::Read(Some(read)),
            dataless: None,
            origin: WriteOrigin::Editor,
            on_conflict: ConflictPolicy::RefuseOnly,
            history: Some(&keep as &dyn Fn(WriteCapture<'_>)),
            lock: Some(&lock),
        },
        Some(&stamp),
    );
    match outcome {
        Ok(_) => {}
        Err(error @ StorageError::SourceChangedOnDisk { .. }) => return Err(Attempt::Moved(error)),
        Err(error) => return Err(Attempt::Stopped(save_failure_message(&error))),
    }

    if let Some(id) = tab.as_deref() {
        let at = resolved.as_deref().map_or(path, Path::new);
        crate::commands::preview::lapse_scripts_opt_in_now(state, id);
        state
            .event_bus
            .emit(crate::watcher::open_files::open_note_modified(
                id,
                at,
                Some(&written),
                encoding,
                Some(writ_core::notes::Writer::Writ),
            ));
    }
    reindex(state, path);
    Ok(CaptureOutcome { name, added: true })
}

/// The file's text and the encoding to write it back in: the tab's own when a
/// tab holds the file (ADR-049), the open's detection otherwise. `None` for
/// bytes that are not text in it.
fn decode_for_append(
    state: &AppState,
    tab: Option<&str>,
    bytes: &[u8],
) -> Option<(TextEncoding, String)> {
    match tab {
        Some(id) => {
            let encoding = crate::watcher::moves::encoding_of_note(state, id).with_bom_of(bytes);
            let text = encoding.decode(bytes).ok()?.into_owned();
            Some((encoding, text))
        }
        None => decode_file(bytes).map(|decoded| (decoded.encoding, decoded.text.into_owned())),
    }
}

/// Mints Today's File holding `entry`, and no tab: the person is in another
/// app, and the file is there for the next time they open the day.
fn mint_with(
    state: &AppState,
    now: DateTime<Utc>,
    path: &Path,
    entry: &str,
) -> Result<CaptureOutcome, Attempt> {
    let root = state.notes_root();
    let stem = writ_core::notes::date_stem(now);
    let extension = state.default_extension();
    let content = append_entry("", entry);
    let keep = keep_versions(&state.note_history);
    let lock = WriteLock::in_data_folder(&state.writ_dir);
    let stamp = ignore_stamper(state);
    let created = create_note_guarded(
        CreateNote {
            notes_root: &root,
            stem: &stem,
            extension: extension.as_str(),
            content: &content,
            origin: WriteOrigin::Editor,
            // Another writer minted the day first: read it and add to it.
            on_taken_name: TakenName::Refuse,
            history: Some(&keep as &dyn Fn(WriteCapture<'_>)),
            lock: Some(&lock),
        },
        Some(&stamp),
    );
    match created {
        Ok(created) => {
            reindex(state, &created);
            Ok(CaptureOutcome {
                name: file_name_only(&created.to_string_lossy()),
                added: true,
            })
        }
        Err(error @ StorageError::NoteNameTaken { .. }) => Err(Attempt::Moved(error)),
        Err(error) => {
            warn!(
                note = %file_name_only(&path.to_string_lossy()),
                %error,
                "quick capture could not mint today's file"
            );
            Err(Attempt::Stopped(save_failure_message(&error)))
        }
    }
}

/// The write was stamped for the watcher, which therefore never reports it, so
/// the index hears of it here.
fn reindex(state: &AppState, path: &Path) {
    if !holds_own_write(state, path) {
        return;
    }
    if let Err(error) = state.notes_index.index_path(path) {
        warn!(
            note = %file_name_only(&path.to_string_lossy()),
            %error,
            "a captured entry could not be indexed"
        );
    }
    announce_indexed(&state.event_bus, [notes_index::index_key(path)]);
}

/// IPC: [`capture_append_inner`], off the main thread (ADR-047).
#[tauri::command]
pub async fn capture_append<R: Runtime>(
    app: AppHandle<R>,
    text: String,
) -> Result<CaptureOutcome, String> {
    run_blocking(app, ERR_WRITE_FAILED, move |state| {
        capture_append_inner(state, Utc::now(), &text)
    })
    .await
}

/// IPC: [`capture_target_inner`].
#[tauri::command]
pub async fn capture_target<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    run_blocking(app, ERR_WRITE_FAILED, |state| {
        capture_target_inner(state, Utc::now())
    })
    .await
}

/// IPC: hides the capture note. With `return_focus`, the app that was in front
/// before it gets the keyboard back. On the main thread, where AppKit wants
/// activation asked for.
#[tauri::command]
pub fn capture_close(app: AppHandle, return_focus: bool) {
    crate::capture::close_from_page(&app, return_focus);
}
