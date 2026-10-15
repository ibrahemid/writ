//! What the app does with a `get_context` ask (ADR-053).
//!
//! `writ mcp` leaves the ask in `open-requests/`, where the requests watcher
//! finds it ([`crate::instance`]). The app checks that the settings let the
//! program see the open file, asks the main window which tab is in front and
//! what is selected in it, resolves that tab's file itself, and leaves the
//! answer in `context-answers/` for that one call. An ask opens no tab and
//! leaves the window where it is; the quick capture window is never asked.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tracing::warn;
use writ_core::buffer::BufferStatus;
use writ_core::config::mcp::McpConfig;
use writ_core::instance::{is_wait_token, ContextAsk};
use writ_core::open_context::{OpenContext, Selected, TextPoint};

use crate::events::{emit_event_to_main, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// How long the window has to answer an ask: as long as `writ mcp` waits.
pub const WINDOW_ANSWER_WAIT: Duration = Duration::from_secs(5);

/// The only window that answers.
pub const MAIN_WINDOW: &str = "main";

/// The asks the main window owes an answer, by token, with when each arrived.
///
/// Held in Tauri's managed state beside `AppState`.
#[derive(Debug, Default)]
pub struct ContextAsks(Mutex<HashMap<String, Instant>>);

impl ContextAsks {
    /// Holds `token` until the window answers it or [`WINDOW_ANSWER_WAIT`]
    /// passes, dropping any ask older than that.
    pub fn hold(&self, token: &str, now: Instant) {
        let mut asks = recover_poison(self.0.lock(), "open_context::hold");
        asks.retain(|_, at| now.duration_since(*at) < WINDOW_ANSWER_WAIT);
        asks.insert(token.to_string(), now);
    }

    /// Takes `token`, answering whether it was asked for and is still waited
    /// on.
    pub fn take(&self, token: &str, now: Instant) -> bool {
        recover_poison(self.0.lock(), "open_context::take")
            .remove(token)
            .is_some_and(|at| now.duration_since(at) < WINDOW_ANSWER_WAIT)
    }
}

/// What the main window answers: the tab in front, by id, and what is
/// selected in it. The file is the app's to name, never the window's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowContext {
    /// The tab in front, or `None` when no tab is.
    #[serde(default)]
    pub buffer_id: Option<String>,
    /// The non-empty selections, in the window's order.
    #[serde(default)]
    pub selections: Vec<Selected>,
    /// Where the caret is.
    #[serde(default)]
    pub cursor: Option<TextPoint>,
    /// The tab holds text its file does not have yet.
    #[serde(default)]
    pub unsaved: bool,
    /// The window left selected text out before sending it.
    #[serde(default)]
    pub truncated: bool,
}

/// Why an answer from the window was not written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContextAnswerError {
    /// The answer came from a window other than the main one.
    #[error("only the main window answers which file is open")]
    NotMainWindow,
    /// Nothing asked for this answer, or the call stopped waiting for it.
    #[error("no program is waiting for this answer")]
    NotAsked,
    /// The answer could not be written to the data folder.
    #[error("the answer could not be written: {0}")]
    NotWritten(String),
}

/// What the app does with one ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskRoute {
    /// The settings do not let the program see the open file: nothing is
    /// asked or answered.
    Refused,
    /// The window is not ready: the answer is that no file is open.
    NothingOpen,
    /// Ask the main window.
    AskWindow,
}

/// Whether `config` lets `program` see the open file.
pub fn is_allowed(config: &McpConfig, program: &str) -> bool {
    config.enabled
        && config
            .approval_for(program)
            .is_some_and(|approval| approval.context)
}

/// What the app does with `ask`, decided from the settings it holds and
/// whether the window is ready.
///
/// `writ mcp` checked the grant before it asked; the app checks it again, so
/// an ask written into the data folder by anything else is answered only for
/// a program the person let see the open file.
pub fn route_ask(state: &AppState, ask: &ContextAsk) -> AskRoute {
    if !is_wait_token(&ask.token) {
        return AskRoute::Refused;
    }
    let allowed = {
        let config = recover_poison(state.config.lock(), "open_context::route_ask");
        is_allowed(&config.mcp, &ask.program)
    };
    if !allowed {
        return AskRoute::Refused;
    }
    if state.frontend_ready.load(Ordering::SeqCst) {
        AskRoute::AskWindow
    } else {
        AskRoute::NothingOpen
    }
}

/// Answers `ask`: asks the main window, answers that no file is open, or,
/// for a program not allowed, does nothing.
pub fn answer_ask(app: &AppHandle, ask: &ContextAsk) {
    let state = app.state::<AppState>();
    match route_ask(&state, ask) {
        AskRoute::Refused => warn!("a context ask for a program not allowed was dropped"),
        AskRoute::NothingOpen => {
            if let Err(error) = writ_storage::open_context::write_answer(
                &state.writ_dir,
                &ask.token,
                &OpenContext::nothing_open(),
            ) {
                warn!(%error, "a context answer could not be written");
            }
        }
        AskRoute::AskWindow => {
            app.state::<ContextAsks>().hold(&ask.token, Instant::now());
            if let Err(error) = emit_event_to_main(
                app,
                WritFrontendEvent::ContextAsked {
                    token: ask.token.clone(),
                },
            ) {
                warn!(%error, "the window could not be asked which file is open");
            }
        }
    }
}

/// The answer the main window sent, as the call is handed it: the tab's file
/// named from the app's own record of it, the selections held to the answer's
/// ceilings.
///
/// A tab that is not open, or holds no file yet, answers no path.
pub fn resolve_answer(state: &AppState, reply: WindowContext) -> OpenContext {
    let path = reply.buffer_id.as_deref().and_then(|id| {
        let store = recover_poison(state.store.lock(), "open_context::resolve_answer");
        store
            .get(id)
            .ok()
            .filter(|doc| doc.status == BufferStatus::Active)
            .and_then(|doc| doc.source_path)
    });
    OpenContext {
        selections: if path.is_some() {
            reply.selections
        } else {
            Vec::new()
        },
        cursor: path.as_ref().and(reply.cursor),
        unsaved: path.is_some() && reply.unsaved,
        truncated: path.is_some() && reply.truncated,
        path,
    }
    .fitted()
}

/// Writes the main window's answer for `token`.
///
/// # Errors
///
/// [`ContextAnswerError`]: the answer came from another window, nothing is
/// waiting for it, or it could not be written.
pub fn answer_inner(
    state: &AppState,
    asks: &ContextAsks,
    window: &str,
    token: &str,
    reply: WindowContext,
) -> Result<(), ContextAnswerError> {
    if window != MAIN_WINDOW {
        return Err(ContextAnswerError::NotMainWindow);
    }
    if !asks.take(token, Instant::now()) {
        return Err(ContextAnswerError::NotAsked);
    }
    let context = resolve_answer(state, reply);
    writ_storage::open_context::write_answer(&state.writ_dir, token, &context)
        .map_err(|error| ContextAnswerError::NotWritten(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ask_is_taken_once_and_not_after_the_wait() {
        let asks = ContextAsks::default();
        let now = Instant::now();
        asks.hold("a-1", now);
        assert!(asks.take("a-1", now));
        assert!(!asks.take("a-1", now), "taken once");

        asks.hold("a-2", now);
        assert!(!asks.take("a-2", now + WINDOW_ANSWER_WAIT));
        assert!(!asks.take("never-asked", now));
    }

    #[test]
    fn an_old_ask_is_dropped_when_a_new_one_arrives() {
        let asks = ContextAsks::default();
        let now = Instant::now();
        asks.hold("old", now);
        asks.hold("new", now + WINDOW_ANSWER_WAIT);
        assert_eq!(recover_poison(asks.0.lock(), "test").len(), 1);
    }
}
