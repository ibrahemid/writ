//! The first launch: the answer it records, the file it opens, and what may
//! rename that file.
//!
//! The policy is [`writ_core::startup`]; this is the mechanism. Nothing is
//! written until the screen is answered (ADR-041 §3), so the work runs from
//! the `finish_first_run` command rather than from Tauri's setup. It records
//! the chosen format, creates the file the window opens on, and it keeps the
//! two facts the retitle guard rails read: whether a note's tab has been
//! closed, and whether anything outside Writ has touched the note's path since
//! it was created.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::info;
use writ_core::buffer::document::BufferDocument;
use writ_core::config::FileExtension;
use writ_core::startup::{retitle_answer, RetitleAnswer, RetitleFacts};

use crate::poison::recover_poison;
use crate::state::AppState;

/// What has happened to each note Writ minted and has not yet retitled.
///
/// A note leaves the table the first time its first line is answered for, so
/// the retitle happens at most once per note however many times it is asked.
#[derive(Debug, Default)]
pub struct RetitleWatch {
    entries: Mutex<HashMap<PathBuf, RetitleFacts>>,
}

impl RetitleWatch {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts watching the note Writ just minted at `path`.
    pub fn watch(&self, path: &Path) {
        let mut entries = recover_poison(self.entries.lock(), "first_run::RetitleWatch::watch");
        entries.insert(path.to_path_buf(), RetitleFacts::default());
    }

    /// Records that the tab holding `path` was closed.
    pub fn closed(&self, path: &Path) {
        self.record(path, |facts| facts.has_been_closed = true);
    }

    /// Records one change to `path` that Writ did not make.
    ///
    /// Every caller is downstream of the watcher's ignore set, so a write Writ
    /// made never reaches here.
    pub fn changed_outside(&self, path: &Path) {
        self.record(path, |facts| {
            facts.watcher_events_seen = facts.watcher_events_seen.saturating_add(1);
        });
    }

    /// What the note's first line may do to the note's file name, or `None`
    /// when the note is not one this applies to: it was not minted here, or it
    /// has already been answered for.
    pub fn answer(&self, path: &Path) -> Option<RetitleAnswer> {
        let entries = recover_poison(self.entries.lock(), "first_run::RetitleWatch::answer");
        entries.get(path).copied().map(retitle_answer)
    }

    /// Stops watching `path`.
    pub fn forget(&self, path: &Path) {
        let mut entries = recover_poison(self.entries.lock(), "first_run::RetitleWatch::forget");
        entries.remove(path);
    }

    /// The facts held for `path`, for tests and for the log line.
    pub fn facts(&self, path: &Path) -> Option<RetitleFacts> {
        let entries = recover_poison(self.entries.lock(), "first_run::RetitleWatch::facts");
        entries.get(path).copied()
    }

    fn record(&self, path: &Path, edit: impl FnOnce(&mut RetitleFacts)) {
        let mut entries = recover_poison(self.entries.lock(), "first_run::RetitleWatch::record");
        if let Some(facts) = entries.get_mut(path) {
            edit(facts);
        }
    }
}

/// Records the format the first launch was answered with, and opens the file
/// that launch shows. `None` on every later launch.
///
/// Nothing is written until this is called, so a person who quits on the
/// screen is asked again next launch and a person whose config is already
/// there is never asked (ADR-041 §3). The choice is written to the config
/// first, because it is what the mint below reads and what makes the next
/// launch a later one (ADR-039 §2).
///
/// Only an empty notes folder is minted into. A folder that already holds
/// files opens the newest of them, so a launch that finds work already done
/// adds nothing to it.
pub fn finish_first_run_inner(
    state: &AppState,
    default_extension: FileExtension,
) -> Result<Option<BufferDocument>, String> {
    if !state.first_run {
        return Ok(None);
    }
    remember_the_launch(state, default_extension)?;
    let note = first_note(state)?;
    info!(opened = note.is_some(), "first launch");
    Ok(note)
}

/// The file this launch opens, or `None` when there is nothing for it to do.
///
/// The order is the one that cannot lose anything: tabs the last session left
/// open take the frontend's ordinary path, any file already in the folder is
/// opened rather than buried under a new empty one, and only an empty folder
/// is minted into.
fn first_note(state: &AppState) -> Result<Option<BufferDocument>, String> {
    if has_open_notes(state)? {
        return Ok(None);
    }
    let root = state.notes_root();
    if let Some(path) = newest_note_in(&root) {
        return open_note_at(state, &path);
    }
    // Minting is what arms the retitle watch, so only a file Writ made this
    // launch can be renamed from its own first line unasked.
    crate::commands::notes::new_note_inner(state).map(Some)
}

/// Whether the last session left tabs for the frontend to restore.
fn has_open_notes(state: &AppState) -> Result<bool, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let open = store
        .list_by_status(writ_core::buffer::document::BufferStatus::Active)
        .map_err(|e| e.to_string())?;
    Ok(!open.is_empty())
}

/// Opens a note that is already on disk.
///
/// A file whose bytes are still in the cloud opens no row, which is not a
/// failure: the launch has nothing to show and says so.
fn open_note_at(state: &AppState, path: &Path) -> Result<Option<BufferDocument>, String> {
    let opened = crate::commands::file::open_file_from_path(state, &path.to_string_lossy())?;
    Ok(opened.doc)
}

/// The note in `root` that was written most recently, or `None` when it holds
/// no notes.
///
/// One level deep, the way [`crate::notes::mint_note_path`] reads the same
/// folder: `src-tauri` carries no directory walker, and a folder holding only
/// subfolders is a shape the mint already treats as empty.
fn newest_note_in(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| is_note_file(path))
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok()
                .unwrap_or(std::time::UNIX_EPOCH)
        })
}

/// Whether `path` is a note rather than one of the files a sync client, an
/// editor or the operating system leaves in a folder.
///
/// The extensions are [`writ_storage::notes_index::has_text_extension`]'s, the
/// list the index already reads, so a folder of `.txt` files is a folder with
/// work in it rather than an empty one to mint into (ADR-041 §2).
fn is_note_file(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'));
    writ_storage::notes_index::has_text_extension(path) && !hidden
}

/// Writes the config this launch found no trace of, carrying the format it was
/// answered with, so the next launch is a later one and mints what was asked
/// for.
///
/// It goes through the same [`crate::commands::config::persist_config`] every
/// command uses, so the file has the shape the next read expects and the write
/// is stamped into the watcher's ignore set. The running copy is replaced
/// after the file lands, so the mint that follows reads the same answer the
/// next launch will.
///
/// A failure stops the launch here rather than minting: a file made in a
/// format the config does not record is the one file the person cannot explain
/// afterwards, and a launch that wrote nothing simply asks again.
fn remember_the_launch(state: &AppState, default_extension: FileExtension) -> Result<(), String> {
    let mut config = recover_poison(state.config.lock(), "first_run::remember_the_launch").clone();
    config.files.default_extension = default_extension;
    crate::commands::config::persist_config(state, &config).map_err(|error| {
        tracing::warn!(
            %error,
            path = %state.config_store.path().display(),
            "first launch could not record the config"
        );
        error
    })?;
    *recover_poison(state.config.lock(), "first_run::remember_the_launch") = config;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        PathBuf::from("/notes/2026-03-18.md")
    }

    #[test]
    fn a_note_nothing_has_touched_is_renamed() {
        let watch = RetitleWatch::new();
        watch.watch(&path());
        assert_eq!(watch.answer(&path()), Some(RetitleAnswer::Rename));
    }

    #[test]
    fn a_closed_tab_and_a_change_outside_both_turn_the_rename_into_a_question() {
        let closed = RetitleWatch::new();
        closed.watch(&path());
        closed.closed(&path());
        assert_eq!(closed.answer(&path()), Some(RetitleAnswer::Ask));

        let changed = RetitleWatch::new();
        changed.watch(&path());
        changed.changed_outside(&path());
        assert_eq!(changed.answer(&path()), Some(RetitleAnswer::Ask));
    }

    #[test]
    fn a_note_that_was_never_minted_here_has_no_answer() {
        let watch = RetitleWatch::new();
        assert_eq!(watch.answer(&path()), None);

        watch.watch(&path());
        watch.forget(&path());
        assert_eq!(watch.answer(&path()), None);
    }

    #[test]
    fn a_change_to_another_note_leaves_this_one_alone() {
        let watch = RetitleWatch::new();
        watch.watch(&path());
        watch.changed_outside(Path::new("/notes/other.md"));
        watch.closed(Path::new("/notes/other.md"));
        assert_eq!(watch.facts(&path()), Some(RetitleFacts::default()));
    }
}
