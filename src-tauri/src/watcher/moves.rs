//! What a watcher does about the file behind a tab moving or going away.
//!
//! The verdict is [`writ_core::notes::identity::classify_delete`]'s; this is
//! the half that acts on it. A move has to reach the note's row before
//! anything else happens, because the row is where the next save reads its
//! destination from: a tab still pointing at the old path recreates a file the
//! user moved, which in a synced folder puts a duplicate on every device
//! (spec W4).
//!
//! It is a trait rather than a direct call because the watcher threads run
//! with a channel and a registry, not with the application. The production
//! implementation reaches the state through the app handle; the tests supply
//! one that records what it was told.

use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Manager};
use writ_core::notes::identity::{FileIdentity, IdentityProbe};

use crate::state::AppState;

use super::identity::PlatformIdentity;

/// The record of a tab's file, and what to do when it moves or goes.
///
/// Every method answers whether the news is new, so the same move seen by both
/// watchers costs the tab one message. A file inside the notes folder that was
/// opened from outside it is watched by both, and the batch deduplication each
/// watcher does is per watcher.
pub trait NoteFiles: Send + Sync {
    /// What the filesystem last called `note_id`'s file.
    fn identity_of(&self, note_id: &str) -> Option<FileIdentity>;

    /// Points the note at `to`, where its file is now. `true` when the row
    /// moved, `false` when it was already there.
    fn note_file_moved(&self, note_id: &str, from: &Path, to: &Path) -> bool;

    /// Records that the note's file was deleted. `true` when this is news.
    fn note_file_removed(&self, note_id: &str, path: &Path) -> bool;

    /// Records what the note's file is now, which is asked every time the tab
    /// hears its file changed: a write another program made replaced the file
    /// under the path, and the tab has to hold the new one's identity or its
    /// own next rename reads as a deletion. `true` when the file had been
    /// marked removed, which is a file that came back from the Trash.
    fn note_file_returned(&self, note_id: &str, path: &Path) -> bool;
}

/// The probe and the record, which every watcher needs together.
#[derive(Clone)]
pub struct FileTracking {
    /// Reads what the filesystem calls a file.
    pub probe: Arc<dyn IdentityProbe>,
    /// Holds what Writ knows about each tab's file.
    pub files: Arc<dyn NoteFiles>,
}

impl FileTracking {
    /// The real probe with nothing behind it: a watcher that classifies moves
    /// but has nowhere to record them. What a test gets, and what production
    /// falls back to if the state is unreachable.
    pub fn untracked() -> Self {
        Self {
            probe: Arc::new(PlatformIdentity),
            files: Arc::new(NoNoteFiles),
        }
    }

    /// Tracking backed by the running application.
    pub fn of_app(app: AppHandle) -> Self {
        Self {
            probe: Arc::new(PlatformIdentity),
            files: Arc::new(AppNoteFiles { app }),
        }
    }

    /// Tracking backed by a state held directly, which is how a test drives
    /// the watchers without an application around them.
    ///
    /// The reference is weak: the state owns the tracking, and a strong one
    /// would keep it alive forever.
    pub fn of_state(state: &Arc<AppState>) -> Self {
        Self {
            probe: Arc::new(PlatformIdentity),
            files: Arc::new(SharedNoteFiles {
                state: Arc::downgrade(state),
            }),
        }
    }
}

/// Nothing recorded and nothing applied. Every verdict is news, so a watcher
/// with no state behind it still tells the tab what it saw.
pub struct NoNoteFiles;

impl NoteFiles for NoNoteFiles {
    fn identity_of(&self, _note_id: &str) -> Option<FileIdentity> {
        None
    }

    fn note_file_moved(&self, _note_id: &str, _from: &Path, _to: &Path) -> bool {
        true
    }

    fn note_file_removed(&self, _note_id: &str, _path: &Path) -> bool {
        true
    }

    fn note_file_returned(&self, _note_id: &str, _path: &Path) -> bool {
        false
    }
}

/// The records the running application keeps.
struct AppNoteFiles {
    app: AppHandle,
}

impl NoteFiles for AppNoteFiles {
    fn identity_of(&self, note_id: &str) -> Option<FileIdentity> {
        identity_of_note(&self.app.state::<AppState>(), note_id)
    }

    fn note_file_moved(&self, note_id: &str, from: &Path, to: &Path) -> bool {
        apply_move(&self.app.state::<AppState>(), note_id, from, to)
    }

    fn note_file_removed(&self, note_id: &str, path: &Path) -> bool {
        apply_removal(&self.app.state::<AppState>(), note_id, path)
    }

    fn note_file_returned(&self, note_id: &str, path: &Path) -> bool {
        apply_return(&self.app.state::<AppState>(), note_id, path)
    }
}

/// The same records reached through a state held directly rather than through
/// an application, which is what a test drives the watchers with.
///
/// Weak, so the state owning the tracking is still droppable. A state that has
/// gone answers nothing, which reads the same as no tab being open.
struct SharedNoteFiles {
    state: std::sync::Weak<AppState>,
}

impl NoteFiles for SharedNoteFiles {
    fn identity_of(&self, note_id: &str) -> Option<FileIdentity> {
        let state = self.state.upgrade()?;
        identity_of_note(&state, note_id)
    }

    fn note_file_moved(&self, note_id: &str, from: &Path, to: &Path) -> bool {
        match self.state.upgrade() {
            Some(state) => apply_move(&state, note_id, from, to),
            None => false,
        }
    }

    fn note_file_removed(&self, note_id: &str, path: &Path) -> bool {
        match self.state.upgrade() {
            Some(state) => apply_removal(&state, note_id, path),
            None => false,
        }
    }

    fn note_file_returned(&self, note_id: &str, path: &Path) -> bool {
        match self.state.upgrade() {
            Some(state) => apply_return(&state, note_id, path),
            None => false,
        }
    }
}

/// What the filesystem last called the note's file.
fn identity_of_note(state: &AppState, note_id: &str) -> Option<FileIdentity> {
    state.source_identity(note_id)
}

/// Moves the note onto `to`: the row, its name, the index entry, the record of
/// what the file held, the write blessing and the folder watch.
///
/// The store lock is released before the folder watch is taken. The save path
/// takes them in that order — a first save attaches a file and then follows it
/// — so taking them the other way round here is how a watcher thread and a
/// keystroke deadlock.
///
/// The bytes did not move, so the digest Writ recorded still describes the
/// file and is carried over rather than read again. Reading again would fetch
/// the whole file in a sync folder for an answer already in hand.
///
/// `false` when the row was already there, which is what keeps one move seen
/// by two watchers from telling the tab twice.
fn apply_move(state: &AppState, note_id: &str, from: &Path, to: &Path) -> bool {
    let Some(destination) = to.to_str() else {
        tracing::warn!(path = %to.display(), "a file moved to a path that cannot be recorded");
        return false;
    };
    {
        let Ok(store) = state.store.lock() else {
            return false;
        };
        let Ok(doc) = store.get(note_id) else {
            return false;
        };
        if doc.source_path.as_deref() == Some(destination) {
            return false;
        }
        if let Err(e) =
            store.rename_to_file(note_id, destination, &crate::commands::notes::note_name(to))
        {
            tracing::warn!(note = %note_id, error = %e, "a note's row could not follow its file");
            return false;
        }
    }
    if let Some(previous) = state.disk_state(note_id) {
        state.record_disk_state(note_id, to, previous.hash, previous.size);
    }
    if let Ok(canonical) = crate::security::canonicalize_for_authorization(to) {
        state.authorized_paths.record_blessed_source(canonical);
    }
    state.follow_note_path(note_id, to);
    state.observe_source_file(note_id, to);
    tracing::info!(
        note = %note_id,
        from = %from.display(),
        to = %to.display(),
        "a tab's file moved and the tab followed it"
    );
    true
}

/// Records that the note's file was deleted. `false` when it was already
/// marked, so one delete costs the tab one message.
fn apply_removal(state: &AppState, note_id: &str, path: &Path) -> bool {
    let news = state.mark_removed_on_disk(note_id);
    if news {
        tracing::info!(
            note = %note_id,
            path = %path.display(),
            "a tab's file was deleted; the tab keeps its text and writes nothing"
        );
    }
    news
}

/// Records what the note's file is now, every time the tab hears its file
/// changed. `true` only when it had been marked removed, which is a file that
/// came back from the Trash.
///
/// The reading happens whatever the answer to that second question is, and
/// that is the point of it. Somebody else's write is a rename over the target
/// on nearly every editor and every sync client, so the file behind the tab is
/// a different file afterwards while the path is unchanged. A tab still
/// holding the id of the replaced file reads its own next rename as a
/// deletion, marks itself removed, and refuses every later save over a file
/// that is sitting at its new path.
fn apply_return(state: &AppState, note_id: &str, path: &Path) -> bool {
    let was_removed = state.is_removed_on_disk(note_id);
    state.observe_source_file(note_id, path);
    if was_removed {
        tracing::info!(
            note = %note_id,
            path = %path.display(),
            "a tab's file is back; the tab is writing to it again"
        );
    }
    was_removed
}
