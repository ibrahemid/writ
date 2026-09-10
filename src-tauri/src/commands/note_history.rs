//! What the version panel asks for: the list, one text, a restore, a copy.
//!
//! Everything here names a note by the version store's own key — an entry id
//! or a folder-relative name — and never by a path from the frontend. The one
//! path a caller does pass is the tab's own source path, and it is only ever
//! used to find that note's entries.
//!
//! A restore is a write like any other, so it goes through
//! [`write_note_guarded`] with the guard's answer intact: a note that changed
//! under Writ is refused and the text being restored is written beside it
//! (ADR-028 §5). It is unstamped, so the tab holding the note hears about it
//! through the folder watcher and reconciles (ADR-033) — which is also what
//! puts the restored text in front of the person who asked for it.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::State;
use writ_core::activity::{ActivityRecord, Actor, Decision};
use writ_core::notes::guard::DiskState;
use writ_core::notes::WriteOrigin;
use writ_storage::buffer_store::BeforeWrite;
use writ_storage::errors::StorageError;
use writ_storage::guarded::{
    keep_versions, write_note_guarded, write_recovered_copy, ConflictPolicy, DiskRead,
    GuardedWrite, WriteCapture,
};
use writ_storage::note_history::NoteHistoryStore;
use writ_storage::paths::file_name_only;

use crate::state::AppState;

/// One text a note held, as the panel lists it.
#[derive(Debug, Clone, Serialize)]
pub struct NoteVersion {
    /// What every other command here takes.
    pub id: i64,
    /// When it was captured, in milliseconds since the epoch, which is what
    /// the panel's date formatting takes.
    pub at_ms: i64,
    /// What the text costs in bytes.
    pub bytes: u64,
}

/// What a restore put back.
#[derive(Debug, Clone, Serialize)]
pub struct RestoredVersion {
    /// The note, folder-relative.
    pub note: String,
    /// What the file holds now.
    pub bytes: u64,
}

/// The file a copy left in the folder.
#[derive(Debug, Clone, Serialize)]
pub struct VersionCopy {
    /// The name of the new file, never the folder it sits in.
    pub name: String,
}

/// Every text the store holds for the note at `path`, newest first.
///
/// A file the notes folder does not hold has no entries rather than an error:
/// the panel opens on whatever tab is in front, and a tab on a file somebody
/// else owns is an empty list, not a failure.
///
/// # Errors
///
/// The index could not be read.
pub fn note_versions_inner(
    store: &NoteHistoryStore,
    path: &Path,
) -> Result<Vec<NoteVersion>, String> {
    let Some(note) = store.key_for(path) else {
        return Ok(Vec::new());
    };
    let entries = store.versions(&note).map_err(|_| {
        format!(
            "{} has no readable history.",
            file_name_only(&path.to_string_lossy())
        )
    })?;
    Ok(entries
        .into_iter()
        .map(|entry| NoteVersion {
            id: entry.id,
            at_ms: millis(entry.at),
            bytes: entry.bytes,
        })
        .collect())
}

/// The text of one entry, for the panel to show.
///
/// # Errors
///
/// The entry is not in the index any more, or its text cannot be read.
pub fn note_version_content_inner(
    store: &NoteHistoryStore,
    version_id: i64,
) -> Result<String, String> {
    let bytes = store.content(version_id).map_err(|e| unreadable(&e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Writes the text of one entry back to its note.
///
/// The write captures before it lands, so what the note held is an entry of
/// its own and restoring is undone by restoring again.
///
/// `last_known` is what Writ last read from the file, from the tab holding it.
/// A note nothing has open passes `None` and the write proceeds: nothing stale
/// was put in front of anybody.
///
/// # Errors
///
/// The entry is not in the index, its text cannot be read, or the note changed
/// on disk since Writ last read it — which names the copy the text went into
/// instead.
pub fn restore_note_version_inner(
    notes_root: &Path,
    writ_dir: &Path,
    store: &NoteHistoryStore,
    version_id: i64,
    last_known: Option<DiskState>,
) -> Result<RestoredVersion, String> {
    let (file, note) = note_file_of(notes_root, store, version_id)?;
    let bytes = store.content(version_id).map_err(|e| unreadable(&e))?;
    let keep = keep_versions(store);
    let written = write_note_guarded(
        GuardedWrite {
            target: &file,
            // Bytes end to end. A restore that went through a string would
            // rewrite line endings and drop a byte order mark, and the text
            // put back would not be the text that was kept.
            bytes: &bytes,
            last_known,
            on_disk: DiskRead::Fresh,
            dataless: None,
            origin: WriteOrigin::Restore,
            on_conflict: ConflictPolicy::RefuseWithCopy,
            history: Some(&keep as &dyn Fn(WriteCapture<'_>)),
        },
        None,
    );

    match written {
        Ok(written) => {
            record(
                writ_dir,
                "restore_note_version",
                &note,
                Decision::Allow,
                Some(written.disk_state.size),
            );
            Ok(RestoredVersion {
                note,
                bytes: written.disk_state.size,
            })
        }
        Err(error) => {
            record(
                writ_dir,
                "restore_note_version",
                &note,
                Decision::Refuse,
                None,
            );
            Err(refusal(&note, &error))
        }
    }
}

/// Writes the text of one entry beside its note as a dated file.
///
/// The way to read an old version without giving up the current one: the note
/// is left exactly as it is.
///
/// `stamp` is the watcher's ignore hook, which the app passes. Both dated
/// copies written into the notes folder take it — this one and the conflict
/// copy in `commands::buffer` — so the folder watcher treats the two the same
/// way rather than announcing one of them as somebody else's file.
///
/// # Errors
///
/// The entry is not in the index, its text cannot be read or is not text, or
/// the file beside the note cannot be written.
pub fn copy_note_version_inner(
    notes_root: &Path,
    writ_dir: &Path,
    store: &NoteHistoryStore,
    version_id: i64,
    now: DateTime<Utc>,
    stamp: BeforeWrite<'_>,
) -> Result<VersionCopy, String> {
    let (file, note) = note_file_of(notes_root, store, version_id)?;
    let bytes = store.content(version_id).map_err(|e| unreadable(&e))?;
    let length = bytes.len() as u64;
    let text = String::from_utf8(bytes).map_err(|_| format!("{note} is not text."))?;
    match write_recovered_copy(&file, &text, now, stamp) {
        Ok(copy) => {
            record(
                writ_dir,
                "copy_note_version",
                &note,
                Decision::Allow,
                Some(length),
            );
            Ok(VersionCopy {
                name: file_name_only(&copy.to_string_lossy()),
            })
        }
        Err(_) => {
            record(writ_dir, "copy_note_version", &note, Decision::Refuse, None);
            Err(format!("The copy of {note} could not be written."))
        }
    }
}

/// Appends one line to the activity log for a write this module made.
///
/// The record is Writ acting on the person's own request (`Actor::App`), and
/// it carries the note's folder-relative name and the length of the text and
/// nothing else: `ActivityRecord` has no field a note's text fits in, so rule
/// 1.7 holds by construction.
///
/// A log that cannot be written is not fatal. The write it describes has
/// already happened, and losing the line does not undo it.
fn record(writ_dir: &Path, action: &str, note: &str, decision: Decision, bytes: Option<u64>) {
    let mut line = ActivityRecord::now(Actor::App, action, decision).with_path(note);
    if let Some(bytes) = bytes {
        line = line.with_bytes(bytes);
    }
    if let Err(error) = writ_storage::activity_log::append(writ_dir, &line) {
        tracing::warn!(error = %error, "the activity log did not take a version record");
    }
}

/// The file an entry belongs to, and the name to put in front of a person.
///
/// The store keeps notes folder-relative, so the path is built here and
/// checked against the folder it was built from: an entry naming anything that
/// resolves outside it is refused rather than written to.
pub fn note_file_of(
    notes_root: &Path,
    store: &NoteHistoryStore,
    version_id: i64,
) -> Result<(PathBuf, String), String> {
    let slug = store.note_of(version_id).map_err(|e| unreadable(&e))?;
    let note = slug.to_string_lossy().into_owned();
    let root =
        crate::security::canonicalize_root(notes_root).unwrap_or_else(|_| notes_root.to_path_buf());
    let candidate = root.join(&slug);
    let outside = || format!("{note} is not in the notes folder.");
    let resolved = crate::security::resolve_for_containment(&candidate).ok_or_else(outside)?;
    if !writ_core::notes::containment::is_inside(&root, Path::new(&resolved)) {
        return Err(outside());
    }
    Ok((PathBuf::from(resolved), note))
}

/// What Writ last read from the file, when a tab is holding it.
fn recorded_state(state: &AppState, file: &Path) -> Option<DiskState> {
    let doc = {
        let store = state.store.lock().ok()?;
        store
            .find_active_by_source_path(&file.to_string_lossy())
            .ok()??
    };
    state.disk_state(&doc.id)
}

/// Milliseconds since the epoch, and zero for a clock before it.
fn millis(at: SystemTime) -> i64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

/// What the panel shows for an entry it cannot read.
///
/// A version that has been retired is the common case and is not a fault, so
/// it reads as one sentence about the entry rather than about the store.
fn unreadable(error: &StorageError) -> String {
    match error {
        StorageError::VersionMissing { .. } => "That version is not here any more.".to_string(),
        _ => "That version could not be read.".to_string(),
    }
}

/// What the panel shows when a restore does not happen.
///
/// `StorageError`'s own Display is written for logs and names the absolute
/// path (`crates/writ-storage/src/errors.rs`). The panel names notes the way
/// the folder does, and the one thing a person needs from a refusal is where
/// the text they asked for went instead.
fn refusal(note: &str, error: &StorageError) -> String {
    match error {
        StorageError::SourceChangedOnDisk { conflict_copy, .. } => match conflict_copy {
            Some(copy) => format!(
                "{note} changed on disk. The version you asked for is beside it in {}.",
                file_name_only(copy)
            ),
            None => format!(
                "{note} changed on disk, and the version you asked for could not be written beside it."
            ),
        },
        _ => format!("{note} was not written."),
    }
}

/// IPC: [`note_versions_inner`].
#[tauri::command]
pub fn note_versions(state: State<'_, AppState>, path: String) -> Result<Vec<NoteVersion>, String> {
    note_versions_inner(&state.note_history, Path::new(&path))
}

/// IPC: [`note_version_content_inner`].
#[tauri::command]
pub fn note_version_content(state: State<'_, AppState>, version_id: i64) -> Result<String, String> {
    note_version_content_inner(&state.note_history, version_id)
}

/// IPC: [`restore_note_version_inner`].
#[tauri::command]
pub fn restore_note_version(
    state: State<'_, AppState>,
    version_id: i64,
) -> Result<RestoredVersion, String> {
    let notes_root = state.notes_root();
    // Resolved twice, here and inside the write, so the write always judges
    // the folder as it stands when it runs. A folder that moved in between
    // leaves `last_known` describing a file the write is not aimed at, which
    // the guard answers by refusing rather than by replacing something it
    // never read.
    let (file, _) = note_file_of(&notes_root, &state.note_history, version_id)?;
    let last_known = recorded_state(&state, &file);
    restore_note_version_inner(
        &notes_root,
        &state.writ_dir,
        &state.note_history,
        version_id,
        last_known,
    )
}

/// IPC: [`copy_note_version_inner`].
#[tauri::command]
pub fn copy_note_version(
    state: State<'_, AppState>,
    version_id: i64,
) -> Result<VersionCopy, String> {
    let notes_root = state.notes_root();
    let stamp = crate::commands::buffer::ignore_stamper(&state);
    copy_note_version_inner(
        &notes_root,
        &state.writ_dir,
        &state.note_history,
        version_id,
        Utc::now(),
        Some(&stamp),
    )
}
