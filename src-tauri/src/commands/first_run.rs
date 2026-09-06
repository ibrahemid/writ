//! The first launch's IPC surface: what to show, and what a first line may do
//! to the name of the note it was typed in.

use serde::Serialize;
use tauri::State;
use writ_core::buffer::document::BufferDocument;
use writ_core::startup::{file_manager_name, RetitleAnswer};

use crate::commands::config::persist_config;
use crate::state::AppState;

/// What the frontend needs to know about this launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirstRunState {
    /// Whether this launch found no config file.
    pub first_run: bool,
    /// Whether the one line under the cursor has already been dismissed.
    pub hint_dismissed: bool,
    /// What this platform calls the app that opens a folder, for the one word
    /// the line substitutes.
    pub file_manager: String,
}

/// [`first_run_state`] without the IPC wrapper.
pub fn first_run_state_inner(state: &AppState) -> Result<FirstRunState, String> {
    let hint_dismissed = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        config.first_run.hint_dismissed
    };
    Ok(FirstRunState {
        first_run: state.first_run,
        hint_dismissed,
        file_manager: host_file_manager().to_string(),
    })
}

/// IPC: [`first_run_state_inner`].
#[tauri::command]
pub fn first_run_state(state: State<'_, AppState>) -> Result<FirstRunState, String> {
    first_run_state_inner(&state)
}

/// [`dismiss_first_run_hint`] without the IPC wrapper.
pub fn dismiss_first_run_hint_inner(state: &AppState) -> Result<(), String> {
    let mut config = {
        let current = state.config.lock().map_err(|e| e.to_string())?;
        current.clone()
    };
    if config.first_run.hint_dismissed {
        return Ok(());
    }
    config.first_run.hint_dismissed = true;
    persist_config(state, &config)?;

    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    *current = config;
    Ok(())
}

/// IPC: the one line under the cursor goes and stays gone.
#[tauri::command]
pub fn dismiss_first_run_hint(state: State<'_, AppState>) -> Result<(), String> {
    dismiss_first_run_hint_inner(&state)
}

/// What the note's first line did to the note's file name.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RetitleOutcome {
    /// The file was renamed. The row that comes back is the note's new state.
    Renamed {
        /// The note, at its new path and under its new title.
        note: BufferDocument,
    },
    /// The rename was not safe to make unasked, so it is offered instead.
    Ask {
        /// The title the offer would apply.
        title: String,
    },
    /// The note is one this applies to, but it has no first line yet. A
    /// later save may still answer.
    NotYet,
    /// The note is not one this applies to: Writ did not mint it this launch,
    /// or its first line has already been answered for.
    Skipped,
}

/// [`auto_retitle_note`] without the IPC wrapper.
pub fn auto_retitle_note_inner(state: &AppState, id: &str) -> Result<RetitleOutcome, String> {
    let path = crate::commands::notes::note_path_for_id(state, id)?;
    let path = std::path::PathBuf::from(path);
    let Some(answer) = state.retitle_watch.answer(&path) else {
        return Ok(RetitleOutcome::Skipped);
    };
    // The file, not the document: the save that led here has already landed,
    // and the file is the only copy of the text (ADR-028).
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let Some(title) = writ_core::startup::first_line_title(&content) else {
        // A note with no first line yet may still name itself on the next
        // save. One whose first line is there and names nothing — a
        // frontmatter fence, a line that is only a link — will not, and it
        // stops being asked about.
        let blank = content.lines().next().unwrap_or_default().trim().is_empty();
        return Ok(if blank {
            RetitleOutcome::NotYet
        } else {
            RetitleOutcome::Skipped
        });
    };
    let title = title.to_string();

    // Once, whichever way it goes: a note whose first line has been answered
    // for is never answered for again, so an edit to the first line an hour
    // later does not move the file under the person a second time.
    state.retitle_watch.forget(&path);

    match answer {
        RetitleAnswer::Rename => {
            let note = crate::commands::notes::rename_note_inner(state, id, &title)?;
            Ok(RetitleOutcome::Renamed { note })
        }
        RetitleAnswer::Ask => Ok(RetitleOutcome::Ask { title }),
    }
}

/// IPC: renames a note Writ minted from the note's own first line, or offers
/// to.
///
/// The rename goes through the same guarded path as the one in the menu, so it
/// is stamped into the watcher's ignore set and recorded on the row in one
/// write. Whether it may happen unasked is
/// [`writ_core::startup::retitle_answer`], and the title is the note's own
/// first line, read from the file the save just wrote.
#[tauri::command]
pub fn auto_retitle_note(state: State<'_, AppState>, id: String) -> Result<RetitleOutcome, String> {
    auto_retitle_note_inner(&state, &id)
}

/// The word this host uses for the app that opens a folder.
///
/// Read from the host's own platform constant, so the frontend never guesses
/// it from the engine's `navigator`. Each host compiles only its own arm; the
/// other two are covered by [`file_manager_name`]'s own test.
pub fn host_file_manager() -> &'static str {
    file_manager_name(crate::startup::HOST_PLATFORM)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arm this host compiles is the one it should have compiled. Each of
    /// the three runs on its own CI leg; the words themselves are
    /// [`file_manager_name`]'s test, which reaches all three from any host.
    #[test]
    fn the_host_names_its_own_file_manager() {
        #[cfg(target_os = "macos")]
        assert_eq!(host_file_manager(), "Finder");
        #[cfg(target_os = "windows")]
        assert_eq!(host_file_manager(), "File Explorer");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(host_file_manager(), "Files");
    }
}
