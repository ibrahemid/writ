use std::path::Path;
use std::time::Instant;

use crate::poison::recover_poison;
use crate::security::{
    canonicalize_for_authorization, paths_equal_for_authorization, resolve_for_containment,
};
use crate::state::AppState;
use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use writ_core::buffer::document::BufferDocument;
use writ_core::buffer::manager::BufferManager;
use writ_core::events::bus::WritEvent;
use writ_core::file_ops::{self, FileOpenMode};
use writ_core::notes::line_ending::LineEnding;
use writ_core::notes::materialise::{decide_open, OpenDecision};
use writ_core::text_encoding::{decode_detected_owned, TextEncoding};
use writ_core::watcher::change_event::ExternalChange;
use writ_storage::buffer_store::BufferStore;
use writ_storage::paths::file_name_only;

const ERR_UNAUTHORIZED_PATH: &str =
    "Writ cannot open that file from here. Use File > Open File, or drop it onto the window.";

/// Code an open carries when its path is in no folder Writ opens from on its
/// own and no gesture of the person's granted it. A path outside those
/// folders answers this whether or not a file is there, so the answer never
/// says what is on disk outside them. The frontend words it
/// (`src/lib/open-failure.ts`); the text after the code is for logs.
pub const ERR_OPEN_NOT_AUTHORIZED: &str = "ERR_OPEN_NOT_AUTHORIZED";

/// Code an open carries when nothing is at a path inside a folder Writ opens
/// from on its own, such as a link to a note not written yet. Saying so there
/// hides nothing, and it is the answer that holds: the way to open a file from
/// elsewhere cannot open one that is not there.
pub const ERR_OPEN_FILE_MISSING: &str = "ERR_OPEN_FILE_MISSING";

/// Returned to the frontend for every `open_file` call.
///
/// Carries the buffer metadata plus the mode tier so the frontend can
/// configure the editor without a second IPC round-trip.
#[derive(Debug, Clone, Serialize)]
pub struct FileOpenResult {
    /// The buffer metadata row, or `None` when nothing was opened: the file
    /// waits on a confirm ([`FileOpenMode::LargeFileConfirm`]), is over the
    /// size Writ opens ([`FileOpenMode::Refused`]), or has its bytes on
    /// another machine ([`FileOpenMode::NotDownloaded`]). Every other mode
    /// carries a row.
    pub doc: Option<BufferDocument>,
    /// How the file was classified.
    pub mode: FileOpenMode,
    /// File size in bytes. For a row it mirrors `doc.size_bytes`; without one
    /// it is the size the confirm states and the refusal names.
    pub size_bytes: u64,
}

// Paths this process treats as sync placeholders regardless of what the
// filesystem says, so the not-downloaded open arm is testable without a real
// provider. Thread-local and dev-build-only, matching
// `crate::preview::protocol`'s recorder: an integration test links the library
// without `cfg(test)`, so `debug_assertions` is what makes the hook reachable
// from `tests/`.
#[cfg(any(test, debug_assertions))]
thread_local! {
    static DATALESS_PATHS: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Makes `canonical` answer as a sync placeholder on this thread. No-op in
/// release builds.
#[cfg(any(test, debug_assertions))]
pub fn mark_dataless_for_test(canonical: &str) {
    DATALESS_PATHS.with(|p| p.borrow_mut().insert(canonical.to_string()));
}

/// Drops every path marked by [`mark_dataless_for_test`] on this thread.
#[cfg(any(test, debug_assertions))]
pub fn clear_dataless_for_test() {
    DATALESS_PATHS.with(|p| p.borrow_mut().clear());
}

/// Whether the file has no local data behind it.
///
/// A stat, never a read: reading is what a placeholder makes expensive, so the
/// question cannot be answered by opening the file.
fn path_is_dataless(path: &Path) -> bool {
    #[cfg(any(test, debug_assertions))]
    {
        let marked =
            DATALESS_PATHS.with(|p| p.borrow().contains(&path.to_string_lossy().to_string()));
        if marked {
            return true;
        }
    }
    writ_storage::notes_index::is_dataless(path)
}

/// Authorizes one open, and says whether a one-shot token paid for it.
///
/// A path inside the workspace, the inbox or the notes folder authorizes
/// itself and can be opened again tomorrow. A path outside all three is opened
/// on a token the person's own gesture recorded, and that token is spent here.
/// A path with nothing at it answers [`ERR_OPEN_FILE_MISSING`] inside the
/// three folders and [`ERR_OPEN_NOT_AUTHORIZED`] outside them.
fn authorize_open(state: &AppState, raw_path: &str) -> Result<(String, bool), String> {
    let canonical = match canonicalize_for_authorization(Path::new(raw_path)) {
        Ok(canonical) => canonical,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(missing_file_answer(state, raw_path));
        }
        Err(_) => return Err(open_not_authorized()),
    };
    if state.authorized_paths.consume_for_open(&canonical) {
        return Ok((canonical, true));
    }
    if state.is_within_workspace(&canonical) {
        return Ok((canonical, false));
    }
    if state.is_within_inbox(&canonical) {
        return Ok((canonical, false));
    }
    if state.is_within_notes(&canonical) {
        return Ok((canonical, false));
    }
    Err(open_not_authorized())
}

fn open_not_authorized() -> String {
    format!("{ERR_OPEN_NOT_AUTHORIZED}: {ERR_UNAUTHORIZED_PATH}")
}

/// The answer for a path with nothing at it. Containment is decided on the
/// path resolved as far as it exists, so a `..` or a link out of a folder
/// answers as a path outside it does.
fn missing_file_answer(state: &AppState, raw_path: &str) -> String {
    let inside = resolve_for_containment(Path::new(raw_path)).is_some_and(|resolved| {
        state.is_within_workspace(&resolved)
            || state.is_within_inbox(&resolved)
            || state.is_within_notes(&resolved)
    });
    if inside {
        format!("{ERR_OPEN_FILE_MISSING}: nothing is at that path")
    } else {
        open_not_authorized()
    }
}

/// Gate on asking a sync provider for a file's bytes.
///
/// A download reads the file, so it is authorized on the same grounds an open
/// is. It differs in one way: it must not spend the pending-open token. That
/// token belongs to the open the frontend performs once the bytes land, and a
/// download that consumed it would leave that open unauthorized.
pub fn authorize_download(state: &AppState, raw_path: &str) -> Result<String, String> {
    let canonical = canonicalize_for_authorization(Path::new(raw_path))
        .map_err(|_| ERR_UNAUTHORIZED_PATH.to_string())?;
    let authorized = state.authorized_paths.is_pending_open(&canonical)
        || state.authorized_paths.is_blessed_source(&canonical)
        || state.is_within_workspace(&canonical)
        || state.is_within_inbox(&canonical)
        || state.is_within_notes(&canonical);
    if authorized {
        Ok(canonical)
    } else {
        Err(ERR_UNAUTHORIZED_PATH.to_string())
    }
}

/// Opens a file from an already-authorized canonical path.
///
/// Reads nothing past the classification for the `LargeFileConfirm` and
/// `Refused` tiers: each answers with its mode and the file's size and no
/// row. A confirm is followed, once the person says Open, by
/// `open_file_confirmed`. A file in the confirm tier that a tab already shows
/// answers that tab instead, since the question is asked before a read.
pub fn open_file_from_path(state: &AppState, path: &str) -> Result<FileOpenResult, String> {
    let (canonical, spent) = authorize_open(state, path)?;
    let opened = open_authorized_path(state, &canonical);
    settle_open_grant(state, &canonical, spent, &opened);
    opened
}

/// Puts a spent one-shot token back when the open it paid for opened no note.
///
/// The grant was for opening this note, and an answer carrying no note has not
/// done that: a refusal, a file waiting to be confirmed, a note whose bytes are
/// still elsewhere. The next attempt is the one the grant was for, and for a
/// path outside every root it is the only thing that can authorize it. Only an
/// open that spent a token puts one back, so no path can be authorized here
/// that was not granted first.
fn settle_open_grant(
    state: &AppState,
    canonical: &str,
    spent: bool,
    opened: &Result<FileOpenResult, String>,
) {
    if spent && !matches!(opened, Ok(result) if result.doc.is_some()) {
        state
            .authorized_paths
            .record_for_open(canonical.to_string());
    }
}

/// The download state for a file whose bytes are not on this machine, or
/// `None` when the file is here and can be read.
///
/// Every open entry point calls this before anything reads the file.
/// `classify_path` sniffs the first bytes for a NUL, and on a placeholder that
/// sniff is what blocks the IPC thread until the provider has fetched the whole
/// file, so the gate has to answer from the stat first.
fn dataless_open_answer(canonical: &str) -> Option<FileOpenResult> {
    let file_path = Path::new(canonical);
    let OpenDecision::Download { .. } = decide_open(file_path, path_is_dataless(file_path)) else {
        return None;
    };
    let size_bytes = std::fs::metadata(file_path).map(|m| m.len()).unwrap_or(0);
    let home = dirs::home_dir();
    let provider =
        crate::startup::sync_provider_for(file_path.parent().unwrap_or(file_path), home.as_deref());
    Some(FileOpenResult {
        mode: FileOpenMode::NotDownloaded {
            path: canonical.to_string(),
            provider,
        },
        size_bytes,
        doc: None,
    })
}

fn open_authorized_path(state: &AppState, canonical: &str) -> Result<FileOpenResult, String> {
    let canonical = canonical.to_string();
    let file_path = Path::new(&canonical);

    if let Some(answer) = dataless_open_answer(&canonical) {
        return Ok(answer);
    }

    let classification = file_ops::classify_path(file_path).map_err(open_failure_message)?;

    if let Some(answer) = unopened_answer(&classification) {
        if classification.mode == FileOpenMode::LargeFileConfirm {
            if let Some(shown) = open_tab_answer(state, &canonical)? {
                return Ok(shown);
            }
        }
        return Ok(answer);
    }

    open_file_classified(
        state,
        &canonical,
        classification.mode,
        classification.size_bytes,
    )
}

/// The answer for a file a tab already shows: that tab's row, in the mode the
/// row gives, with the tab brought back in line with the file. `None` when no
/// tab shows it.
fn shown_tab_answer(
    state: &AppState,
    store: &BufferStore,
    canonical: &str,
) -> Result<Option<FileOpenResult>, String> {
    let Some(existing) = store
        .find_active_by_source_path(canonical)
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    state
        .authorized_paths
        .record_blessed_source(canonical.to_string());
    resync_open_buffer(state, store, &existing);
    let mode = file_ops::classify_file(existing.size_bytes, existing.read_only).once_open();
    Ok(Some(FileOpenResult {
        mode,
        size_bytes: existing.size_bytes,
        doc: Some(existing),
    }))
}

/// The answer for a file in the confirm tier that a tab shows: that tab's
/// row, in the mode the row gives. `None` when no tab shows it.
///
/// Nothing reads the file or follows it again: the tab has been followed since
/// it opened, and an open of the tab's own path, which the frontend answers
/// without Rust, costs nothing either. A tab restored at launch whose file
/// Writ has not read answers through its own open, which measures the file
/// and asks first (ADR-019).
fn open_tab_answer(state: &AppState, canonical: &str) -> Result<Option<FileOpenResult>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let Some(existing) = store
        .find_active_by_source_path(canonical)
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    state
        .authorized_paths
        .record_blessed_source(canonical.to_string());
    let mode = file_ops::classify_file(existing.size_bytes, existing.read_only).once_open();
    Ok(Some(FileOpenResult {
        mode,
        size_bytes: existing.size_bytes,
        doc: Some(existing),
    }))
}

/// The answer for a tier that opens nothing yet, or `None` for one that opens.
///
/// A refusal and a question are both answers: the mode says which, the size is
/// what the frontend states, and no row exists. Nothing past the
/// classification has read the file.
fn unopened_answer(classification: &file_ops::FileClassification) -> Option<FileOpenResult> {
    match classification.mode {
        FileOpenMode::Refused { .. } | FileOpenMode::LargeFileConfirm => Some(FileOpenResult {
            mode: classification.mode.clone(),
            size_bytes: classification.size_bytes,
            doc: None,
        }),
        _ => None,
    }
}

/// An open that could not read the file, as the read code the editor's own
/// read carries ([`crate::commands::buffer::io_read_failure_code`]) followed
/// by the message for logs.
fn open_failure_message(error: writ_core::errors::WritError) -> String {
    match error {
        writ_core::errors::WritError::Io(io) => io_read_failure_message(&io),
        other => other.to_string(),
    }
}

fn io_read_failure_message(error: &std::io::Error) -> String {
    format!(
        "{}: {error}",
        crate::commands::buffer::io_read_failure_code(error.kind())
    )
}

/// Registers the file and records what it holds.
///
/// Reached from `open_file` for a tier that opens straight away and from
/// `open_file_confirmed` once the person said Open. The path is authorized by
/// the caller.
fn open_file_classified(
    state: &AppState,
    canonical: &str,
    mode: FileOpenMode,
    size_bytes: u64,
) -> Result<FileOpenResult, String> {
    let opened = open_file_classified_inner(state, canonical, mode, size_bytes)?;
    // A file opened from anywhere but the notes folder is followed from here
    // on, so an edit another program makes to it reaches the tab instead of
    // being found the hard way on the next save. An answer carrying no note is
    // not an open: a note still waiting on its bytes has no id to follow, and
    // giving the watcher one would have it report a file the download is about
    // to rewrite.
    if let Some(doc) = &opened.doc {
        state.follow_note_file(doc);
    }
    Ok(opened)
}

/// [`open_file_classified`] without the watch, which is the whole of the open.
fn open_file_classified_inner(
    state: &AppState,
    canonical: &str,
    mode: FileOpenMode,
    size_bytes: u64,
) -> Result<FileOpenResult, String> {
    let file_path = Path::new(canonical);
    let store = state.store.lock().map_err(|e| e.to_string())?;

    if let Some(shown) = shown_tab_answer(state, &store, canonical)? {
        return Ok(shown);
    }

    let is_binary = matches!(mode, FileOpenMode::Binary);

    // The digest is carried alongside the content rather than recomputed from
    // it: for the binary tier `content` is a hex dump, not the file's bytes,
    // and for every tier re-reading a large file just to hash it would defeat
    // the point of the large-file tiers existing at all.
    //
    // A binary file is opened read-only as a hex dump, so there is no ending
    // or encoding in it to keep; the row can never be written back either.
    //
    // Text is read in the encoding detected from its bytes (ADR-049), which
    // the row keeps beside the ending, both read off the decoded text so a
    // UTF-16 file's CRLF counts as CRLF.
    let bytes = std::fs::read(file_path).map_err(|e| io_read_failure_message(&e))?;
    let digest = writ_core::hash::sha256_bytes(&bytes);
    let bytes_prefix = bytes[..bytes.len().min(file_ops::BINARY_CHECK_BYTES)].to_vec();
    let (content, line_ending, encoding) = if is_binary {
        (
            file_ops::generate_hex_dump(&bytes, size_bytes as usize),
            LineEnding::default(),
            None,
        )
    } else {
        let (encoding, text) = decode_detected_owned(bytes);
        let line_ending = LineEnding::detect(&text);
        (text, line_ending, Some(encoding))
    };

    if let Some(history_buf) = store
        .find_history_by_source_path(canonical)
        .map_err(|e| e.to_string())?
    {
        store.restore(&history_buf.id).map_err(|e| e.to_string())?;
        // A file the row keeps as a hex view because it opened as one before
        // UTF-16 was text (ADR-049) opens as text now. A NUL is what made it
        // binary then; a document Writ generated is read-only without one,
        // and stays read-only.
        if history_buf.read_only && !is_binary && bytes_prefix.contains(&0) {
            store
                .set_read_only(&history_buf.id, false)
                .map_err(|e| e.to_string())?;
        }
        if !is_binary {
            store
                .set_line_ending(&history_buf.id, line_ending)
                .map_err(|e| e.to_string())?;
            store
                .set_encoding(&history_buf.id, encoding)
                .map_err(|e| e.to_string())?;
        }
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
            doc: Some(doc),
        });
    }

    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let new_doc = mgr
        .open_external(canonical.to_string())
        .map_err(|e| e.to_string())?;

    let new_doc = BufferDocument {
        read_only: is_binary,
        size_bytes,
        line_ending,
        encoding,
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
        doc: Some(new_doc),
    })
}

#[tauri::command]
pub fn open_file(state: State<'_, AppState>, path: String) -> Result<FileOpenResult, String> {
    open_file_from_path(&state, &path)
}

/// Opens a file in the 50-500 MiB tier once the person said Open.
///
/// Classifies again, since the file can change while the question is up: a
/// file now over the limit answers `Refused`, and one in the confirm tier
/// opens as a large file.
#[tauri::command]
pub fn open_file_confirmed(
    state: State<'_, AppState>,
    path: String,
) -> Result<FileOpenResult, String> {
    open_file_confirmed_from_path(&state, &path)
}

/// [`open_file_confirmed`] against an `AppState` rather than a Tauri handle.
pub fn open_file_confirmed_from_path(
    state: &AppState,
    path: &str,
) -> Result<FileOpenResult, String> {
    let (canonical, spent) = authorize_open(state, path)?;
    let opened = open_confirmed_path(state, &canonical);
    settle_open_grant(state, &canonical, spent, &opened);
    opened
}

fn open_confirmed_path(state: &AppState, canonical: &str) -> Result<FileOpenResult, String> {
    // Ahead of `classify_path`, on the same grounds as `open_authorized_path`:
    // the file can be evicted while the confirmation dialog is up, and the
    // sniff would then fetch the whole of it on the IPC thread.
    if let Some(answer) = dataless_open_answer(canonical) {
        return Ok(answer);
    }

    let file_path = Path::new(canonical);
    let classification = file_ops::classify_path(file_path).map_err(open_failure_message)?;
    if let FileOpenMode::Refused { .. } = classification.mode {
        return Ok(FileOpenResult {
            mode: classification.mode,
            size_bytes: classification.size_bytes,
            doc: None,
        });
    }
    open_file_classified(
        state,
        canonical,
        classification.mode.once_open(),
        classification.size_bytes,
    )
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
                tracing::warn!(
                    error = %e,
                    path = %file_name_only(&pb.to_string_lossy()),
                    "failed to canonicalize dialog path; skipping"
                );
            }
        }
    }
    Ok(out)
}

/// IPC: asks the person where a note with no file goes ("Save As…" on the
/// bar of a note whose first file could not be made), and answers the path
/// they chose, or `None` when they cancelled.
///
/// The panel opens where [`suggest_note_save_path`] says, and the path chosen
/// is answered through [`accept_note_save_path`].
#[tauri::command]
pub async fn pick_note_save_path(
    app: tauri::AppHandle,
    id: String,
) -> Result<Option<String>, String> {
    let (folder, name) = suggest_note_save_path(&app.state::<AppState>(), &id)?;

    let (tx, rx) = std::sync::mpsc::channel::<Option<tauri_plugin_dialog::FilePath>>();
    app.dialog()
        .file()
        .set_title("Save As")
        .set_directory(&folder)
        .set_file_name(name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let Some(picked) = rx.recv().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let picked = picked.into_path().map_err(|e| e.to_string())?;
    accept_note_save_path(&app.state::<AppState>(), &picked).map(Some)
}

/// Where the save panel opens for note `id`: the notes folder, with the name
/// the note's first save gives it.
pub fn suggest_note_save_path(
    state: &AppState,
    id: &str,
) -> Result<(std::path::PathBuf, String), String> {
    let notes_root = state.notes_root();
    // Read before the store lock: the config lock is never taken under it.
    let extension = state.default_extension();
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    let suggested =
        crate::notes::mint_note_path(&notes_root, &doc.title, chrono::Utc::now(), extension);
    let name = suggested
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((notes_root, name))
}

/// Takes the path the person chose in the save panel: resolved the way the
/// write gate resolves a path, and recorded as one the person picked, so the
/// write that follows ([`crate::commands::buffer::save_note_to`]) may land
/// outside the notes folder. Answers the resolved path.
pub fn accept_note_save_path(state: &AppState, picked: &Path) -> Result<String, String> {
    let resolved = crate::security::resolve_for_containment(picked)
        .ok_or_else(|| format!("{} cannot be written to", picked.display()))?;
    state
        .authorized_paths
        .record_blessed_source(resolved.clone());
    Ok(resolved)
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
    // The tab's encoding stands: it is what the open detected or what Reopen
    // with Encoding chose, and opening the file again chooses nothing.
    let encoding = doc.encoding.unwrap_or_else(TextEncoding::utf8);
    if !doc.read_only {
        if let Ok(text) = encoding.decode(&source) {
            let ending = LineEnding::detect(&text);
            if ending != doc.line_ending {
                if let Err(e) = store.set_line_ending(&doc.id, ending) {
                    tracing::debug!(buffer_id = %doc.id, error = %e, "the file's line ending could not be recorded");
                }
            }
        }
    }
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
    // A change is only reportable against a record of what the file held. A
    // note Writ has not read this launch has none, and saying it changed would
    // be an assertion about bytes nobody looked at. It lands on the tab least
    // able to answer for itself: one restored at launch and never brought to
    // the front, whose editor is not mounted and which reads the file itself
    // the moment it is. Silence costs nothing there and a claim costs a prompt
    // over a document nobody touched.
    let Some(known) = state.disk_state(&doc.id) else {
        return;
    };
    if known.hash != writ_core::hash::sha256_bytes(&source) {
        // Another program changed the file, so an opt-in given to the bytes
        // Writ read before goes with them (ADR-043). It lapses here, under the
        // guard this function is handed: the bus runs its subscribers on this
        // thread with that guard held, so none of them may take the store.
        crate::commands::preview::lapse_scripts_opt_in(store, &state.preview_render_cache, &doc.id);
        // The digest is the file's own, computed from the bytes just read, in
        // the form the editor compares its document against. A reopen is the
        // one place the file has already been read, so leaving it out and
        // making the editor go back to disk would be an IPC round trip for an
        // answer already in hand.
        state.event_bus.emit(WritEvent::BufferExternal {
            buffer_id: doc.id.clone(),
            path: doc.source_path.clone().unwrap_or_default(),
            change: ExternalChange::Modified,
            new_path: None,
            disk_hash: Some(encoding.comparison_digest_hex(&source)),
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
            doc: Some(existing),
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
            doc: Some(doc),
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
        doc: Some(new_doc),
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
/// A path that no longer exists passes, and that is deliberately not the same
/// question as whether the file was deleted. This gate asks whether a write
/// may be aimed at a path at all; a path resolves whether or not anything is
/// there, which is what lets a new note be minted and a note whose folder was
/// remade be saved. Nothing can be redirected in that case either, since
/// [`write_atomic`](writ_storage::atomic::write_atomic) renames onto the
/// literal path rather than following a dangling link.
///
/// Whether the file behind an open tab was deleted is asked one layer up, by
/// [`crate::commands::buffer::save_buffer_content_inner`], which knows which
/// note the write is for and can therefore read what the watcher recorded
/// about that note's file. A note marked removed on disk is refused there
/// under `ERR_FILE_REMOVED_ON_DISK` rather than recreated (spec W4).
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
    match crate::security::resolve_for_containment(Path::new(source_path)) {
        Some(resolved) => state.is_within_notes(&resolved),
        None => false,
    }
}
