//! What the app does with a program's held write (ADR-053).
//!
//! `writ mcp` leaves the proposal in `write-reviews/` and the ask in
//! `open-requests/`, where the requests watcher finds it ([`crate::instance`]).
//! The app checks that the settings still hold that program's writes for the
//! person, shows the change in the main window and brings it forward, and
//! leaves the person's answer in `review-answers/`. Anything but a clear
//! Apply from the main window, while the write still waits and the program
//! may still write, is answered as a discard or as not shown, so nothing
//! lands that the person did not apply.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tracing::warn;
use writ_core::config::mcp::McpConfig;
use writ_core::diff::{line_diff_exact, Hunk};
use writ_core::instance::{is_wait_token, ReviewAsk};
use writ_core::write_review::{ProposedChange, ReviewAnswer, WriteProposal, REVIEW_WAIT};

use crate::events::{emit_event_to_main, WritFrontendEvent};
use crate::open_context::MAIN_WINDOW;
use crate::poison::recover_poison;
use crate::state::AppState;

/// How much sooner than `writ mcp` the app stops taking an answer, so an
/// Apply never lands in a file the program has stopped looking for.
pub const REVIEW_MARGIN: Duration = Duration::from_secs(2);

/// How long a held write sent at `sent_at_ms` still takes an answer at
/// `now_ms`: what is left of [`REVIEW_WAIT`], less [`REVIEW_MARGIN`].
pub fn remaining_wait(sent_at_ms: i64, now_ms: i64) -> Duration {
    let elapsed = u64::try_from(now_ms.saturating_sub(sent_at_ms)).unwrap_or(0);
    REVIEW_WAIT
        .saturating_sub(Duration::from_millis(elapsed))
        .saturating_sub(REVIEW_MARGIN)
}

/// The held writes the main window owes an answer, by token, with whose each
/// is and until when it takes one.
///
/// Held in Tauri's managed state beside `AppState`.
#[derive(Debug, Default)]
pub struct ReviewAsks(Mutex<HashMap<String, (String, Instant)>>);

impl ReviewAsks {
    /// Holds `token` for `program` for `wait` from `now`, dropping any whose
    /// wait has passed.
    pub fn hold(&self, token: &str, program: &str, wait: Duration, now: Instant) {
        let mut asks = recover_poison(self.0.lock(), "write_review::hold");
        asks.retain(|_, (_, until)| now < *until);
        asks.insert(token.to_string(), (program.to_string(), now + wait));
    }

    /// Takes `token`, answering the program whose write it holds while that
    /// write still waits.
    pub fn take(&self, token: &str, now: Instant) -> Option<String> {
        recover_poison(self.0.lock(), "write_review::take")
            .remove(token)
            .filter(|(_, until)| now < *until)
            .map(|(program, _)| program)
    }
}

/// What the main window shows of one held write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownReview {
    /// What the window answers with.
    pub token: String,
    /// The program, by the name it sent.
    pub program: String,
    /// The file, relative to the notes folder, or its name alone outside it.
    pub path: String,
    /// `edit`, `create` or `rename`.
    pub kind: &'static str,
    /// The name a rename would give the file.
    pub rename_to: Option<String>,
    /// What the write would change, line by line.
    pub hunks: Vec<Hunk>,
    /// How long the write still waits for the answer, in milliseconds.
    pub wait_ms: u64,
}

/// Why an answer from the window was not written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewAnswerError {
    /// The answer came from a window other than the main one.
    #[error("only the main window answers a held write")]
    NotMainWindow,
    /// Nothing holds this write, or the program stopped waiting for it.
    #[error("no program is waiting for this answer")]
    NotAsked,
    /// The answer could not be written to the data folder.
    #[error("the answer could not be written: {0}")]
    NotWritten(String),
}

/// What the app does with one held write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRoute {
    /// The settings do not hold this program's writes, or the token could
    /// name another file: answered as not shown.
    NotHeld,
    /// The window is not ready: answered as not shown.
    NotReady,
    /// Show it in the main window.
    Show,
}

/// Whether `config` holds `program`'s writes for the person.
pub fn is_held(config: &McpConfig, program: &str) -> bool {
    config.enabled
        && config
            .approval_for(program)
            .is_some_and(|approval| approval.reviews_writes())
}

/// What the app does with `ask`, from the settings it holds and whether the
/// window is ready. `writ mcp` checked the setting before it asked; the app
/// checks it again, so a request anything else left is never shown.
pub fn route_review(state: &AppState, ask: &ReviewAsk) -> ReviewRoute {
    if !is_wait_token(&ask.token) {
        return ReviewRoute::NotHeld;
    }
    let held = {
        let config = recover_poison(state.config.lock(), "write_review::route_review");
        is_held(&config.mcp, &ask.program)
    };
    if !held {
        ReviewRoute::NotHeld
    } else if state.frontend_ready.load(Ordering::SeqCst) {
        ReviewRoute::Show
    } else {
        ReviewRoute::NotReady
    }
}

/// `proposal` as the window shows it, waiting `wait` more, or `None` when it
/// is not `program`'s or is too large to compare.
pub fn shown_review(
    notes_root: &Path,
    ask: &ReviewAsk,
    proposal: &WriteProposal,
    wait: Duration,
) -> Option<ShownReview> {
    if proposal.program != ask.program {
        return None;
    }
    let (kind, rename_to, hunks) = match &proposal.change {
        ProposedChange::Edit { before, after } => {
            ("edit", None, line_diff_exact(before, after).ok()?)
        }
        ProposedChange::Create { after } => ("create", None, line_diff_exact("", after).ok()?),
        ProposedChange::Rename { to } => ("rename", Some(to.clone()), Vec::new()),
    };
    Some(ShownReview {
        token: ask.token.clone(),
        program: ask.program.clone(),
        path: spelled_for_window(notes_root, Path::new(&proposal.path)),
        kind,
        rename_to,
        hunks,
        wait_ms: u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
    })
}

/// A file as the window names it: relative to the notes folder inside it, its
/// name alone anywhere else.
fn spelled_for_window(notes_root: &Path, file: &Path) -> String {
    let root = writ_core::notes::containment::resolve_for_containment(notes_root)
        .unwrap_or_else(|| notes_root.to_path_buf());
    writ_storage::paths::relative_slug(&root, file)
        .filter(|relative| !relative.is_empty())
        .unwrap_or_else(|| writ_storage::paths::file_name_only(&file.to_string_lossy()))
}

fn answer_not_shown(writ_dir: &Path, token: &str) {
    if let Err(error) =
        writ_storage::write_review::write_answer(writ_dir, token, ReviewAnswer::NotShown)
    {
        warn!(%error, "a held write could not be answered");
    }
}

/// Shows `ask`, sent at `sent_at_ms`, in the main window and brings it
/// forward, or answers that it was not shown.
pub fn show_review(app: &AppHandle, ask: &ReviewAsk, sent_at_ms: i64, now_ms: i64) {
    let state = app.state::<AppState>();
    let wait = remaining_wait(sent_at_ms, now_ms);
    if wait.is_zero() {
        answer_not_shown(&state.writ_dir, &ask.token);
        return;
    }
    match route_review(&state, ask) {
        ReviewRoute::NotHeld => {
            warn!("a held write for a program not set to ask first was answered as not shown");
            if is_wait_token(&ask.token) {
                answer_not_shown(&state.writ_dir, &ask.token);
            }
        }
        ReviewRoute::NotReady => answer_not_shown(&state.writ_dir, &ask.token),
        ReviewRoute::Show => {
            let shown = writ_storage::write_review::read_proposal(&state.writ_dir, &ask.token)
                .and_then(|proposal| shown_review(&state.notes_root(), ask, &proposal, wait));
            let Some(shown) = shown else {
                answer_not_shown(&state.writ_dir, &ask.token);
                return;
            };
            app.state::<ReviewAsks>()
                .hold(&ask.token, &ask.program, wait, Instant::now());
            if let Err(error) = emit_event_to_main(app, WritFrontendEvent::ReviewAsked(shown)) {
                warn!(%error, "a held write could not be shown");
                app.state::<ReviewAsks>().take(&ask.token, Instant::now());
                answer_not_shown(&state.writ_dir, &ask.token);
                return;
            }
            crate::instance::bring_forward(app);
        }
    }
}

/// Writes the main window's answer for `token`: Apply only when the person
/// applied it and the program may still write, a discard otherwise.
///
/// # Errors
///
/// [`ReviewAnswerError`]: the answer came from another window, nothing waits
/// for it, or it could not be written.
pub fn answer_inner(
    state: &AppState,
    asks: &ReviewAsks,
    window: &str,
    token: &str,
    apply: bool,
) -> Result<(), ReviewAnswerError> {
    if window != MAIN_WINDOW {
        return Err(ReviewAnswerError::NotMainWindow);
    }
    let program = asks
        .take(token, Instant::now())
        .ok_or(ReviewAnswerError::NotAsked)?;
    let still_held = {
        let config = recover_poison(state.config.lock(), "write_review::answer_inner");
        is_held(&config.mcp, &program)
    };
    let answer = if apply && still_held {
        ReviewAnswer::Apply
    } else {
        ReviewAnswer::Discard
    };
    writ_storage::write_review::write_answer(&state.writ_dir, token, answer)
        .map_err(|error| ReviewAnswerError::NotWritten(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_write_is_taken_once_and_not_after_it_stopped_waiting() {
        let asks = ReviewAsks::default();
        let now = Instant::now();
        asks.hold("r-1", "codex", REVIEW_WAIT, now);
        assert_eq!(asks.take("r-1", now).as_deref(), Some("codex"));
        assert_eq!(asks.take("r-1", now), None, "taken once");

        asks.hold("r-2", "codex", REVIEW_WAIT, now);
        assert_eq!(asks.take("r-2", now + REVIEW_WAIT), None);
        assert_eq!(asks.take("never-held", now), None);
    }

    #[test]
    fn the_app_stops_taking_an_answer_before_the_program_stops_waiting() {
        let waited = i64::try_from(REVIEW_WAIT.as_millis()).expect("fits");
        assert_eq!(remaining_wait(1_000, 1_000), REVIEW_WAIT - REVIEW_MARGIN);
        assert_eq!(
            remaining_wait(1_000, 61_000),
            REVIEW_WAIT - Duration::from_secs(60) - REVIEW_MARGIN
        );
        assert_eq!(remaining_wait(0, waited), Duration::ZERO);
        assert_eq!(remaining_wait(0, waited + 10_000), Duration::ZERO);
        assert_eq!(
            remaining_wait(5_000, 1_000),
            REVIEW_WAIT - REVIEW_MARGIN,
            "a clock moved back takes nothing off"
        );
    }

    #[test]
    fn a_change_is_shown_relative_to_the_folder_and_only_as_its_programs() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(notes.join("plans")).expect("folder");
        let root = writ_core::notes::containment::resolve_for_containment(&notes).expect("root");
        let ask = ReviewAsk {
            token: "r-3".to_string(),
            program: "claude-code".to_string(),
        };
        let proposal = WriteProposal {
            program: "claude-code".to_string(),
            path: root
                .join("plans")
                .join("Plan.md")
                .to_string_lossy()
                .into_owned(),
            change: ProposedChange::Edit {
                before: "one\n".to_string(),
                after: "one\ntwo\n".to_string(),
            },
        };
        let shown = shown_review(&notes, &ask, &proposal, REVIEW_WAIT).expect("shown");
        assert_eq!(shown.path, "plans/Plan.md");
        assert_eq!(shown.kind, "edit");
        assert_eq!(shown.hunks.len(), 1);

        let elsewhere = WriteProposal {
            path: "/somewhere/else/Other.md".to_string(),
            ..proposal.clone()
        };
        assert_eq!(
            shown_review(&notes, &ask, &elsewhere, REVIEW_WAIT)
                .expect("shown")
                .path,
            "Other.md"
        );

        let someone_elses = WriteProposal {
            program: "codex".to_string(),
            ..proposal
        };
        assert_eq!(
            shown_review(&notes, &ask, &someone_elses, REVIEW_WAIT),
            None
        );
    }
}
