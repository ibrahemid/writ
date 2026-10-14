//! Running a command's work off the main thread (ADR-047).
//!
//! Tauri runs a command that is not `async` inline on the main thread, so a
//! command that reads, hashes or writes a file holds the window for as long as
//! the file takes. The IO-bound commands are `async` and hand their work to the
//! blocking pool through [`run_blocking`]; the main thread only dispatches them.
//! The `_inner` functions they wrap stay synchronous, which is what the tests
//! and the other callers in this crate call.

use tauri::{AppHandle, Manager, Runtime};
use writ_core::serial::{Ticket, Turn};

use crate::state::AppState;

/// Why a command's work never answered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OffloadError {
    /// The work panicked, or the runtime shut down before it ran. Carries the
    /// failure code of the command it stood in for, so the editor reads it
    /// the way it reads that command's other failures.
    #[error("{code}: the command stopped before it answered ({reason})")]
    Stopped {
        /// The command's own failure code.
        code: &'static str,
        /// What the runtime reported.
        reason: String,
    },
}

/// Failure code for a command with none of its own.
pub const ERR_COMMAND_STOPPED: &str = "ERR_COMMAND_STOPPED";

/// Runs `work` against the app's state on the blocking pool and answers with
/// what it returned.
///
/// `code` is the failure code the answer carries if the work never returns
/// one ([`OffloadError::Stopped`]).
pub async fn run_blocking<R, T, F>(
    app: AppHandle<R>,
    code: &'static str,
    work: F,
) -> Result<T, String>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(|error| stopped(code, &error))?
}

/// [`run_blocking`] for work on one note's file, run once every command that
/// took a place in that note's queue before `ticket` has finished
/// ([`AppState::buffer_writes`]).
///
/// The ticket is taken by the caller before the work is handed to the pool,
/// so commands on one note keep the order they began in even when the pool
/// starts their work in another.
pub async fn run_in_turn<R, T, F>(
    app: AppHandle<R>,
    ticket: Ticket,
    code: &'static str,
    work: F,
) -> Result<T, String>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce(&AppState, &Turn) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let turn = ticket.wait();
        work(&app.state::<AppState>(), &turn)
    })
    .await
    .map_err(|error| stopped(code, &error))?
}

/// [`run_blocking`] for work that needs nothing of the app's state.
pub async fn run_detached<T, F>(code: &'static str, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| stopped(code, &error))?
}

/// Takes the next place in note `id`'s queue.
pub fn note_ticket<R: Runtime>(app: &AppHandle<R>, id: &str) -> Ticket {
    app.state::<AppState>().buffer_writes.ticket(id)
}

fn stopped(code: &'static str, error: &dyn std::fmt::Display) -> String {
    OffloadError::Stopped {
        code,
        reason: error.to_string(),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_handed_to_the_pool_answers_with_what_it_returned() {
        let answer = tauri::async_runtime::block_on(run_detached("ERR_TEST", || Ok(7)));
        assert_eq!(answer, Ok(7));
        let refused: Result<(), String> =
            tauri::async_runtime::block_on(run_detached("ERR_TEST", || Err("no".to_string())));
        assert_eq!(refused, Err("no".to_string()));
    }

    #[test]
    fn work_that_panics_answers_with_the_command_code() {
        let answer: Result<(), String> =
            tauri::async_runtime::block_on(run_detached("ERR_TEST", || panic!("the work failed")));
        assert!(answer.unwrap_err().starts_with("ERR_TEST: "));
    }

    #[test]
    fn a_stopped_command_answers_with_its_own_code() {
        let message = stopped("ERR_WRITE_FAILED", &"task panicked");
        assert!(message.starts_with("ERR_WRITE_FAILED: "));
        assert!(message.contains("task panicked"));
    }
}
