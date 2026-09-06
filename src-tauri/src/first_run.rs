//! The first launch: the note it opens, and what may rename that note.
//!
//! The policy is [`writ_core::startup`]; this is the mechanism. It creates the
//! note the window opens on, and it keeps the two facts the retitle guard
//! rails read: whether a note's tab has been closed, and whether anything
//! outside Writ has touched the note's path since it was created.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::info;
use writ_core::buffer::document::BufferDocument;
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

/// Creates and opens the note the first launch shows, or `None` on every
/// later launch.
///
/// Nothing is asked: no folder picker, no theme, no account. The notes folder
/// is already there — [`AppState::initialize`] resolves and creates it before
/// anything can write into it — and the note is named for today, so the tab
/// carries a date rather than a placeholder.
pub fn open_first_note(state: &AppState) -> Option<BufferDocument> {
    if !state.first_run {
        return None;
    }
    match crate::commands::notes::new_dated_note_inner(state) {
        Ok(doc) => {
            info!("first launch opened a note");
            Some(doc)
        }
        Err(error) => {
            tracing::warn!(%error, "first launch could not open a note");
            None
        }
    }
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
