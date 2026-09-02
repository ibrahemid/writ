//! Creating, renaming, trashing and copying notes.
//!
//! Notes are managed from inside Writ and the files follow (ADR-028 §3). Each
//! command here moves a real file first and touches the row second, so a
//! failure leaves the note on disk rather than leaving a row pointing at
//! nothing. The disk half is [`writ_storage::note_ops`]; the policy half —
//! what a typed name becomes — is [`writ_core::notes`].

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;
use tracing::{info, warn};
use writ_core::buffer::document::BufferDocument;
use writ_core::buffer::manager::BufferManager;
use writ_core::notes::{rename_stem, NotesRootRefusal};
use writ_storage::buffer_store::BufferStore;
use writ_storage::errors::{StorageError, StorageResult};
use writ_storage::note_ops;
use writ_storage::notes_migration::{self, MigrationReport};
use writ_storage::notes_move;

use crate::commands::buffer::{ignore_stamper, save_failure_message};
use crate::security::canonicalize_for_authorization;
use crate::state::{AppState, NotesRootFallback};

/// What the editor says when a rename arrives with nothing in it.
const NAME_IS_EMPTY: &str = "That name is empty.";

/// The name a note is known by: the file's own name, extension included, which
/// is what Finder shows and what the tab shows.
fn note_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The file the note lives in, or the failure to hand back when it has none.
fn note_path(doc: &BufferDocument) -> Result<String, String> {
    doc.source_path
        .clone()
        .ok_or_else(|| format!("note {} has no file yet", doc.id))
}

/// Renders a note operation's failure for the editor.
///
/// The two the user can act on get a sentence naming what to do about them.
/// A stopped rename carries the same stable code a stopped save does, so the
/// editor has one place that answers "the file changed underneath you".
fn note_failure_message(error: &StorageError) -> String {
    match error {
        StorageError::NoteNameEmpty => NAME_IS_EMPTY.to_string(),
        StorageError::NoteNameTaken { name, .. } => {
            format!("A note named \"{name}\" is already there.")
        }
        other => save_failure_message(other),
    }
}

/// Creates a note, file first, and opens it.
///
/// The file exists in the notes folder before this returns, which is what
/// `New Note` promises: it is in Finder immediately, not on the first
/// keystroke and not at quit (ADR-028 §3). The first-keystroke mint stays for
/// a note that somehow has no file, which after this is only one a recovery
/// pass produced.
pub fn new_note_inner(state: &AppState) -> Result<BufferDocument, String> {
    let now = chrono::Utc::now();
    let stem = writ_core::notes::note_file_stem("", now);

    let store = state.store.lock().map_err(|e| e.to_string())?;
    let stamp = ignore_stamper(state);
    let path = note_ops::create_note(&state.notes_root(), &stem, Some(&stamp))
        .map_err(|e| note_failure_message(&e))?;
    let canonical = canonicalize_for_authorization(&path).map_err(|e| e.to_string())?;

    let mut mgr = BufferManager::new().with_event_bus(state.event_bus.clone());
    let doc = mgr
        .open_external(canonical.clone())
        .map_err(|e| e.to_string())?;
    store.open_from_path(&doc, "").map_err(|e| e.to_string())?;

    state.authorized_paths.record_blessed_source(canonical);
    state.record_disk_state_bytes(&doc.id, &path, b"");
    Ok(doc)
}

/// IPC: [`new_note_inner`].
#[tauri::command]
pub fn new_note(state: State<'_, AppState>) -> Result<BufferDocument, String> {
    new_note_inner(&state)
}

/// Renames a note's file and the row that points at it.
///
/// The move goes through the same guard a save does, so a rename cannot carry
/// a file another process rewrote out from under Writ
/// ([`note_ops::rename_note`]). The row's file and title are then written
/// together, and the tab keeps its content, cursor and undo history because
/// the note's id never changes and the editor is never reloaded.
pub fn rename_note_inner(
    state: &AppState,
    id: &str,
    title: &str,
) -> Result<BufferDocument, String> {
    rename_note_recording(state, id, title, &record_rename)
}

/// How a rename records itself on the row.
///
/// A seam, so the compensating rename-back can be exercised: the failure it
/// answers is a database write failing between the file moving and the row
/// following it, which nothing else can provoke.
pub type RecordRename<'a> = &'a dyn Fn(&BufferStore, &str, &str, &str) -> StorageResult<()>;

/// The real recording: the row's file and title in one write.
fn record_rename(
    store: &BufferStore,
    id: &str,
    source_path: &str,
    title: &str,
) -> StorageResult<()> {
    store.rename_to_file(id, source_path, title)
}

/// [`rename_note_inner`] with the row write handed in.
///
/// The file moves before the row does, so there is a moment where the row
/// names a file that is no longer there. If the row write fails, the file is
/// renamed back: leaving it moved would leave a note whose row points at
/// nothing, which is a note nobody can open again. The compensating move is
/// stamped like any other, and a failure of *it* is reported alongside the
/// original, because at that point the two disagree and only a person can say
/// which is right.
pub fn rename_note_recording(
    state: &AppState,
    id: &str,
    title: &str,
    record: RecordRename<'_>,
) -> Result<BufferDocument, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    let from = note_path(&doc)?;
    let from = Path::new(&from);

    let stem = rename_stem(from, title).ok_or_else(|| NAME_IS_EMPTY.to_string())?;

    let stamp = ignore_stamper(state);
    let to = note_ops::rename_note(from, &stem, state.disk_state(id), Some(&stamp))
        .map_err(|e| note_failure_message(&e))?;
    let to_text = to
        .to_str()
        .ok_or_else(|| format!("the file name {} cannot be recorded", to.display()))?;

    if let Err(error) = record(&store, id, to_text, &note_name(&to)) {
        return Err(undo_rename(&to, from, &stamp, error));
    }

    if let Ok(canonical) = canonicalize_for_authorization(&to) {
        state.authorized_paths.record_blessed_source(canonical);
    }
    // The bytes did not move, so the digest still describes the file; only the
    // metadata around it is re-read. Nothing is recorded for a note Writ has
    // not read this launch, which leaves its first save to read the file.
    if let Some(previous) = state.disk_state(id) {
        state.record_disk_state(id, &to, previous.hash, previous.size);
    }

    store.get(id).map_err(|e| e.to_string())
}

/// Puts a renamed file back under the name its row still holds.
fn undo_rename(
    to: &Path,
    from: &Path,
    stamp: &impl Fn(&Path, &[u8]),
    cause: StorageError,
) -> String {
    let bytes = std::fs::read(to).unwrap_or_default();
    stamp(to, &bytes);
    stamp(from, &bytes);
    match std::fs::rename(to, from) {
        Ok(()) => {
            tracing::warn!(
                path = %to.display(),
                error = %cause,
                "the row could not follow the rename; the file was put back"
            );
            cause.to_string()
        }
        Err(undo) => {
            tracing::error!(
                path = %to.display(),
                error = %cause,
                undo = %undo,
                "the row could not follow the rename and the file could not be put back"
            );
            format!("{cause}; the file is now at {}", to.display())
        }
    }
}

/// IPC: [`rename_note_inner`].
#[tauri::command]
pub fn rename_note(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<BufferDocument, String> {
    rename_note_inner(&state, &id, &title)
}

/// What the editor says when a delete names a file the notes folder does not
/// hold.
const NOT_YOURS_TO_DELETE: &str =
    "Only notes in your notes folder can be moved to the Trash from here.";

/// Moves a note to the operating system's trash and drops its row.
///
/// Only a file inside the notes folder. A tab can hold a file opened from
/// anywhere — a colleague's repository, a mounted volume, a file the user is
/// reading and not keeping — and a Delete on the tab bar has to mean "delete
/// my note", never "delete somebody's file". The file the tab came from is
/// closed rather than deleted; the containment check is the same resolution
/// the write gate runs, so a symlink cannot carry a delete out of the folder.
///
/// The row goes only after the file has gone, so a platform that will not take
/// the file leaves the note both on disk and in Writ rather than leaving a row
/// pointing at a file nobody can reach. A note with no file yet has nothing to
/// trash and only loses its row.
pub fn delete_note_inner(state: &AppState, id: &str) -> Result<(), String> {
    let source_path = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let doc = store.get(id).map_err(|e| e.to_string())?;
        doc.source_path
    };

    if let Some(path) = source_path.as_deref() {
        if !path_is_inside_notes(state, path) {
            return Err(NOT_YOURS_TO_DELETE.to_string());
        }
        let path = Path::new(path);
        note_ops::trash_note(path)
            .map_err(|_| format!("Couldn't move \"{}\" to the Trash.", note_name(path)))?;
    }

    crate::commands::buffer::delete_buffer_inner(state, id)
}

/// IPC: [`delete_note_inner`].
#[tauri::command]
pub fn delete_note(state: State<'_, AppState>, id: String) -> Result<(), String> {
    delete_note_inner(&state, &id)
}

/// Writes `content` into the notes folder as a new note, leaving the file it
/// came from untouched, and returns the path written.
///
/// This is how a file opened from anywhere else earns a place among the notes
/// without moving. The caller opens the returned path, which is inside the
/// notes folder and so needs no further permission.
pub fn save_note_copy_inner(state: &AppState, id: &str, content: &str) -> Result<String, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    let stem = writ_core::notes::note_file_stem(&copy_stem(&doc), chrono::Utc::now());

    let stamp = ignore_stamper(state);
    let path = note_ops::save_copy(&state.notes_root(), &stem, content, Some(&stamp))
        .map_err(|e| note_failure_message(&e))?;
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("the file name {} cannot be recorded", path.display()))
}

/// The name a copy starts from: the file's own stem when it has one, so
/// `report.md` copies to `report.md` and not `report.md.md`, and the title
/// otherwise.
fn copy_stem(doc: &BufferDocument) -> String {
    doc.source_path
        .as_deref()
        .map(Path::new)
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| doc.title.clone())
}

/// IPC: [`save_note_copy_inner`].
#[tauri::command]
pub fn save_note_copy(
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<String, String> {
    save_note_copy_inner(&state, &id, &content)
}

/// The notes folder as text.
pub fn notes_root_text(state: &AppState) -> String {
    state.notes_root().to_string_lossy().into_owned()
}

/// IPC: the absolute path of the notes folder.
///
/// The sidebar reads it to know which rows are notes, and Settings shows it as
/// the answer to "where are my notes".
#[tauri::command]
pub fn get_notes_root(state: State<'_, AppState>) -> String {
    notes_root_text(&state)
}

/// The file note `id` lives in, as its row records it.
pub fn note_path_for_id(state: &AppState, id: &str) -> Result<String, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = store.get(id).map_err(|e| e.to_string())?;
    note_path(&doc)
}

/// IPC: shows a note in the platform's file manager.
///
/// The folder is the product's answer to "where are my notes", so getting to
/// it has to be one click from the note itself.
#[tauri::command]
pub fn show_note_in_file_manager(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let path = note_path_for_id(&state, &id)?;
    crate::commands::storage::show_in_file_manager(Path::new(&path))
}

/// IPC: shows a file inside the notes folder in the platform's file manager.
///
/// The sidebar names a path rather than a note, because a row it lists need
/// not be open. Only a path the notes folder contains is accepted: the webview
/// picks the argument, and a file manager launched on any path it names is a
/// way out of the app's own boundaries. Containment is decided on the resolved
/// path, so a symlink cannot carry the answer out of the folder, which is the
/// same resolution the write gate runs.
///
/// A folder is opened rather than selected, which is what the report's link to
/// `Recovered/` wants: the point of that link is the files inside it.
#[tauri::command]
pub fn show_notes_file_in_file_manager(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    if !path_is_inside_notes(&state, &path) {
        return Err(ERR_NOT_A_NOTE.to_string());
    }
    let path = Path::new(&path);
    if path.is_dir() {
        return crate::commands::storage::open_folder_in_file_manager(path);
    }
    crate::commands::storage::show_in_file_manager(path)
}

/// Code a request carries when it names a path the notes folder does not hold.
///
/// The frontend never shows this: the entry it comes from is only drawn on a
/// row inside the folder, so reaching it means something else asked.
pub const ERR_NOT_A_NOTE: &str = "ERR_NOT_A_NOTE";

/// Whether `path` is a file the notes folder contains.
///
/// Resolution is the write gate's: every existing part of the path is
/// canonicalised first, so neither the file nor a linked directory above it
/// can carry an answer out of the folder.
pub fn path_is_inside_notes(state: &AppState, path: &str) -> bool {
    match crate::security::resolve_for_containment(Path::new(path)) {
        Some(resolved) => state.is_within_notes(&resolved),
        None => false,
    }
}

/// Where the notes folder is, what to call it, and whether it is the one the
/// user asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotesFolderInfo {
    /// The absolute path, which is what `Copy path` puts on the clipboard.
    pub path: String,
    /// The same path with the home folder collapsed to `~`, which is what
    /// Settings shows (ADR-028 section 2).
    pub display_path: String,
    /// The folder the settings named and why the notes are not in it, when
    /// startup fell back to the default. `None` on every ordinary launch.
    pub fallback: Option<NotesRootFallback>,
    /// The sync service whose folder the notes are in, as the user knows it,
    /// or `None` when they are not in one. Writ syncs nothing itself, so this
    /// is the whole answer to "are my notes on my other machine".
    pub sync_provider: Option<String>,
}

/// [`get_notes_folder`] without the IPC wrapper.
pub fn notes_folder_info(state: &AppState) -> NotesFolderInfo {
    let root = state.notes_root();
    let home = dirs::home_dir();
    NotesFolderInfo {
        display_path: writ_core::notes::display_path(&root, home.as_deref()),
        sync_provider: crate::startup::sync_provider_for(&root, home.as_deref()),
        path: root.to_string_lossy().into_owned(),
        fallback: state.notes_root_fallback(),
    }
}

/// IPC: [`notes_folder_info`].
#[tauri::command]
pub fn get_notes_folder(state: State<'_, AppState>) -> NotesFolderInfo {
    notes_folder_info(&state)
}

/// IPC: opens the notes folder in the platform's file manager.
#[tauri::command]
pub fn show_notes_folder_in_finder(state: State<'_, AppState>) -> Result<(), String> {
    crate::commands::storage::open_folder_in_file_manager(&state.notes_root())
}

/// What moving the notes folder did.
///
/// `collided` is non-empty only when nothing moved and `new_root` is still the
/// old folder: the destination already held those names, and a note is never
/// written over to make room for another (ADR-028 section 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoveNotesOutcome {
    /// The notes folder in force after the call.
    pub new_root: String,
    /// Entries moved out of the old folder.
    pub moved: usize,
    /// Names the destination already held.
    pub collided: Vec<String>,
}

/// What Writ says when the folder picked would swallow its own data folder.
const WOULD_HOLD_WRIT_DATA: &str =
    "Writ keeps its own data in that folder, so it cannot also be your notes folder.";

/// What Writ says when the folder picked is inside the one being moved.
const WOULD_HOLD_ITSELF: &str = "Pick a folder outside your notes folder.";

/// What Writ says when the folder picked cannot be resolved to a path it can
/// ask questions about: it is written relative to a working directory, or it
/// ends in a `..` that names no folder. The OS folder dialog yields neither.
const CANNOT_BE_CHECKED: &str = "That folder cannot be your notes folder.";

/// Writ's own data folder in the spelling a canonical path can be compared
/// against.
///
/// `writ_dir` is kept as it was configured, which on macOS means `/var/...`
/// where the resolved path is `/private/var/...`. Comparing the two forms
/// answers "no" for a folder that does hold the database, so the refusal below
/// would never fire. A folder that cannot be resolved falls back to its own
/// spelling: it does not exist yet, so it holds nothing either way.
fn canonical_writ_dir(state: &AppState) -> PathBuf {
    crate::security::canonicalize_root(&state.writ_dir).unwrap_or_else(|_| state.writ_dir.clone())
}

/// Applies [`writ_core::notes::refuse_notes_root`] and puts the answer in the
/// words Settings shows.
fn refuse_destination(state: &AppState, from: &Path, candidate: &Path) -> Result<(), String> {
    match writ_core::notes::refuse_notes_root(candidate, from, &canonical_writ_dir(state)) {
        Some(NotesRootRefusal::HoldsWritData) => Err(WOULD_HOLD_WRIT_DATA.to_string()),
        Some(NotesRootRefusal::InsideNotesFolder) => Err(WOULD_HOLD_ITSELF.to_string()),
        None => Ok(()),
    }
}

/// Moves the notes folder to `destination` and points Writ at it.
///
/// One operation, in the order that leaves nothing half done: the files move
/// first, then the config and this process learn where they went, then every
/// row that named a file under the old folder is rewritten. A move that
/// collides moves nothing and says which names clashed.
///
/// What is re-recorded is the write guard's memory of each open note: its key
/// is the note, but the record describes a file at a path that no longer
/// exists, so it is dropped and taken again from the file in its new home.
pub fn move_notes_folder_to(
    state: &AppState,
    destination: &Path,
) -> Result<MoveNotesOutcome, String> {
    let from = state.notes_root();

    // Asked first of the folder the pick will resolve to, so a pick Writ turns
    // down does not leave an empty folder behind at the path it named. A path
    // that will not resolve is turned down here rather than created and asked
    // about afterwards: the answer would arrive one folder too late.
    let Some(planned) = crate::security::resolve_for_containment(destination) else {
        return Err(CANNOT_BE_CHECKED.to_string());
    };
    refuse_destination(state, &from, Path::new(&planned))?;

    std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let to = crate::security::canonicalize_root(destination).map_err(|e| e.to_string())?;

    if to == from {
        return Ok(MoveNotesOutcome {
            new_root: path_text(&from),
            moved: 0,
            collided: Vec::new(),
        });
    }
    refuse_destination(state, &from, &to)?;

    let outcome = notes_move::move_notes_folder(&from, &to).map_err(|e| e.to_string())?;
    if !outcome.collided.is_empty() {
        return Ok(MoveNotesOutcome {
            new_root: path_text(&from),
            moved: 0,
            collided: outcome.collided,
        });
    }

    // Before any row moves, so no walk that started against the old folder is
    // still running when they do: it would finish and prune every row this
    // move re-keys.
    state.notes_index.bump_generation();

    let text = path_text(&to);
    adopt_notes_root(state, &to, &text)?;
    repoint_notes(state, &from, &to)?;
    follow_notes_root(state, &from, &to);

    Ok(MoveNotesOutcome {
        new_root: text,
        moved: outcome.moved,
        collided: Vec::new(),
    })
}

/// Writes the new folder into the config and into this process.
///
/// The config is written first: a folder Writ forgets on the next launch is
/// worse than one it has not started using yet, and every note is already in
/// it by the time this runs.
fn adopt_notes_root(state: &AppState, root: &Path, text: &str) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    let next = writ_core::config::WritConfig {
        notes: writ_core::config::NotesConfig {
            root: Some(text.to_string()),
        },
        ..config.clone()
    };
    crate::commands::config::persist_config(state, &next)?;
    *config = next;
    drop(config);

    state.set_notes_root(root.to_path_buf());
    // The store decides on its own whether a save belongs to the notes folder,
    // so it needs the same answer as the state: a root captured at startup
    // would stop indexing the moment the folder moved.
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .set_notes_root(root.to_path_buf());
    Ok(())
}

/// Brings the notes index and the notes watcher to the folder Writ now uses.
///
/// Nothing here can fail the move: the files are already in their new home, so
/// a failure is logged and left to the walk this spawns. The order matters.
/// The index is re-keyed first, which is what makes the following walk read
/// nothing; the watcher is armed next, before the walk, so a file that lands
/// while the walk runs is still seen; the walk goes last and covers everything
/// that preceded it.
fn follow_notes_root(state: &AppState, from: &Path, to: &Path) {
    match state.notes_index.rekey_root(from, to) {
        Ok(rekeyed) => info!(rekeyed, "notes index followed the folder"),
        Err(e) => warn!(error = %e, "notes index could not follow the folder"),
    }

    match crate::watcher::handler::start_notes_watcher(
        state.event_bus.clone(),
        to.to_path_buf(),
        state.watcher_ignore.clone(),
    ) {
        Ok(handle) => {
            let mut slot = state
                .notes_watcher
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // Assigning drops the old handle, which closes its channel and
            // ends the thread watching the folder Writ left.
            *slot = Some(handle);
        }
        Err(e) => warn!(error = %e, "notes watcher could not follow the folder"),
    }

    let index = state.notes_index.clone();
    let cancel = state.notes_index_cancel.clone();
    let generation = index.generation();
    let root = to.to_path_buf();
    std::thread::spawn(move || {
        let cancelled = || {
            cancel.load(std::sync::atomic::Ordering::Relaxed) || index.generation() != generation
        };
        match index.reconcile(&root, &cancelled, &writ_storage::notes_index::is_dataless) {
            Ok(outcome) => info!(
                added = outcome.added,
                updated = outcome.updated,
                removed = outcome.removed,
                cancelled = outcome.cancelled,
                "notes index reconciled after the move"
            ),
            Err(e) => warn!(error = %e, "notes index reconcile after the move failed"),
        }
    });
}

/// Points every row and every disk-state record at the moved files.
fn repoint_notes(state: &AppState, from: &Path, to: &Path) -> Result<(), String> {
    let moved = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        notes_move::repoint_rows(&store, from, to).map_err(|e| e.to_string())?
    };

    for row in moved {
        let path = PathBuf::from(&row.to);
        if let Ok(canonical) = canonicalize_for_authorization(&path) {
            state.authorized_paths.record_blessed_source(canonical);
        }
        // Only a note Writ has read or written this launch has a record to
        // move. Reading every file in the folder to make one would be the one
        // thing this operation must not do: in a sync folder it pulls down
        // every note that is not on this machine (ADR-028 section 7).
        if state.disk_state(&row.id).is_none() {
            continue;
        }
        state.forget_disk_state(&row.id);
        if let Ok(bytes) = std::fs::read(&path) {
            state.record_disk_state_bytes(&row.id, &path, &bytes);
        }
    }
    Ok(())
}

/// IPC: asks for a folder and moves the notes into it.
///
/// `Ok(None)` when the dialog was dismissed.
#[tauri::command]
pub async fn pick_notes_folder(app: tauri::AppHandle) -> Result<Option<MoveNotesOutcome>, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = std::sync::mpsc::channel::<Option<tauri_plugin_dialog::FilePath>>();
    app.dialog()
        .file()
        .set_title("Choose a notes folder")
        .pick_folder(move |path| {
            let _ = tx.send(path);
        });

    let picked = rx.recv().map_err(|e| e.to_string())?;
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let destination = file_path.into_path().map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    move_notes_folder_to(&state, &destination).map(Some)
}

/// [`get_notes_migration_report`] without the IPC wrapper.
pub fn notes_migration_report(state: &AppState) -> Result<Option<MigrationReport>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    notes_migration::report_to_show(&store).map_err(|e| e.to_string())
}

/// IPC: the report the notes migration left, while it still has one to show.
#[tauri::command]
pub fn get_notes_migration_report(
    state: State<'_, AppState>,
) -> Result<Option<MigrationReport>, String> {
    notes_migration_report(&state)
}

/// [`dismiss_notes_migration_report`] without the IPC wrapper.
pub fn dismiss_notes_migration_report_inner(state: &AppState) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    notes_migration::dismiss_report(&store, chrono::Utc::now()).map_err(|e| e.to_string())
}

/// IPC: [`dismiss_notes_migration_report_inner`]. The report is shown once.
#[tauri::command]
pub fn dismiss_notes_migration_report(state: State<'_, AppState>) -> Result<(), String> {
    dismiss_notes_migration_report_inner(&state)
}

/// What emptying the archive into the notes folder did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoveArchiveOutcome {
    /// Notes that arrived in the notes folder.
    pub moved: usize,
    /// The names that were already taken, so the note arrived under a
    /// numbered one.
    pub collided: Vec<String>,
}

/// [`move_archived_notes`] without the IPC wrapper.
pub fn move_archived_notes_inner(state: &AppState) -> Result<MoveArchiveOutcome, String> {
    let archive = state.writ_dir.join("archive");
    let notes = state.notes_root();

    let store = state.store.lock().map_err(|e| e.to_string())?;
    let outcome = notes_move::move_archive_into_notes(&store, &archive, &notes, chrono::Utc::now())
        .map_err(|e| e.to_string())?;
    drop(store);

    Ok(MoveArchiveOutcome {
        moved: outcome.moved,
        collided: outcome.collided,
    })
}

/// IPC: moves the notes the migration archived into the notes folder.
///
/// The archive is where a closed note's text waited until the user asked for
/// it, because writing a hundred of them into a folder that may be syncing is
/// the user's call to make (ADR-028 section 4 step 3). This is that answer.
#[tauri::command]
pub fn move_archived_notes(state: State<'_, AppState>) -> Result<MoveArchiveOutcome, String> {
    move_archived_notes_inner(&state)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_name_is_named_back() {
        let error = StorageError::NoteNameTaken {
            name: "Grocery list.md".to_string(),
            folder: std::path::PathBuf::from("/notes"),
        };
        assert_eq!(
            note_failure_message(&error),
            "A note named \"Grocery list.md\" is already there."
        );
    }

    #[test]
    fn an_empty_name_is_said_plainly() {
        assert_eq!(
            note_failure_message(&StorageError::NoteNameEmpty),
            NAME_IS_EMPTY
        );
    }
}
