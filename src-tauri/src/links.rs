//! What the app does with a `writ://` link (ADR-052).
//!
//! A link reaches the app four ways: the operating system's open-URL event on
//! macOS, the arguments of a launch on Windows and Linux, a second launch's
//! hand-over, and a request `writ mcp` leaves (U23's open-requests folder).
//! All four end in [`follow`], so one reading of the grammar decides.
//!
//! `open` resolves its path inside the notes folder as it stands now and goes
//! the way a forwarded path goes ([`crate::instance::route_forwarded`]):
//! authorized, parked until the frontend is ready, shown at its line. `new`
//! asks the frontend for a new tab, which reaches a file on its first
//! keystroke (ADR-028 section 2), so following a link writes nothing.
//!
//! A refused link is logged by its reason alone: the link may name a file.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tracing::{info, warn};
use writ_core::instance::OpenRequest;
use writ_core::writ_link::{self, LinkRefusal, WritLink};

use crate::events::{emit_event, emit_event_to_main, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// Why a link asked for nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// Outside the grammar.
    #[error(transparent)]
    Grammar(#[from] LinkRefusal),
    /// An `open` whose path is not a file the notes folder holds, once
    /// resolved on disk.
    #[error("the link names no file in the folder")]
    NotInFolder,
}

/// What one link asks for, once read and resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkRoute {
    /// Open this file, resolved inside the notes folder, at this line.
    Open {
        /// The file, as the folder spells it on disk.
        path: String,
        /// The 1-based line.
        line: Option<u32>,
    },
    /// A new tab, called this when there is a name.
    Draft(Option<String>),
}

/// What a batch of links asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FollowedLinks {
    /// The files to open, with their lines, as a forwarded request.
    pub opens: OpenRequest,
    /// The new tabs to open, in order.
    pub drafts: Vec<Option<String>>,
}

/// New tabs links asked for before the frontend could open them.
///
/// Held in Tauri's managed state beside `AppState`; drained when the frontend
/// says it is ready.
#[derive(Debug, Default)]
pub struct LinkDrafts(Mutex<Vec<Option<String>>>);

/// Reads `raw` and resolves it against `notes_root`.
///
/// # Errors
///
/// [`LinkError`]: the link is outside the grammar, or names no file the
/// folder holds.
pub fn route_link(notes_root: &Path, raw: &str) -> Result<LinkRoute, LinkError> {
    match writ_link::parse(raw)? {
        WritLink::Open { path, line } => {
            let file = writ_storage::paths::file_in_folder(notes_root, &path)
                .ok_or(LinkError::NotInFolder)?;
            Ok(LinkRoute::Open {
                path: file.to_string_lossy().into_owned(),
                line,
            })
        }
        WritLink::New { name } => Ok(LinkRoute::Draft(name)),
    }
}

/// Reads every link in `links` against `notes_root`, logging each refusal by
/// its reason.
pub fn read_links(notes_root: &Path, links: &[String]) -> FollowedLinks {
    let mut followed = FollowedLinks::default();
    for raw in links {
        match route_link(notes_root, raw) {
            Ok(LinkRoute::Open { path, line }) => {
                if let Some(line) = line {
                    followed.opens.lines.insert(path.clone(), line);
                }
                followed.opens.paths.push(path);
            }
            Ok(LinkRoute::Draft(name)) => followed.drafts.push(name),
            Err(error) => warn!(reason = %error, "a writ:// link was refused"),
        }
    }
    followed
}

/// Follows `links`: opens the files they name and the new tabs they ask for,
/// bringing the window forward when `reveal` is set and a link asked for
/// something.
///
/// Before the frontend is ready, both are parked and delivered when it is.
pub fn follow(app: &AppHandle, links: &[String], reveal: bool) {
    if links.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    let followed = read_links(&state.notes_root(), links);
    let asked = !followed.opens.paths.is_empty() || !followed.drafts.is_empty();
    if !followed.opens.paths.is_empty() {
        info!(
            count = followed.opens.paths.len(),
            "files opened from links"
        );
        if let Some(opens) = crate::instance::route_forwarded(&state, &followed.opens) {
            let _ = emit_event(
                app,
                WritFrontendEvent::PendingOpens {
                    paths: opens.paths,
                    lines: opens.lines,
                },
            );
        }
    }
    if !followed.drafts.is_empty() {
        let parked = app.state::<LinkDrafts>();
        if let Some(drafts) = park_or_pass(&parked, &state.frontend_ready, followed.drafts) {
            emit_drafts(app, drafts);
        }
    }
    if reveal && asked {
        crate::instance::bring_forward(app);
    }
}

/// Hands the frontend the new tabs links asked for before it was ready.
///
/// Called once the ready flag is set, so a link arriving from here on is
/// emitted rather than parked.
pub fn drain_drafts(app: &AppHandle) {
    let drafts = std::mem::take(&mut *recover_poison(
        app.state::<LinkDrafts>().0.lock(),
        "links::drain_drafts",
    ));
    emit_drafts(app, drafts);
}

/// Parks `drafts` until the frontend is ready, or hands them back to send now.
///
/// The flag is read under the lock [`drain_drafts`] takes after the flag is
/// set, so a draft is either parked before the drain or sent after it.
pub fn park_or_pass(
    parked: &LinkDrafts,
    ready: &std::sync::atomic::AtomicBool,
    drafts: Vec<Option<String>>,
) -> Option<Vec<Option<String>>> {
    let mut held = recover_poison(parked.0.lock(), "links::park_or_pass");
    if ready.load(Ordering::SeqCst) {
        return Some(drafts);
    }
    held.extend(drafts);
    None
}

/// What is parked now, for a test to read.
pub fn parked(parked: &LinkDrafts) -> Vec<Option<String>> {
    recover_poison(parked.0.lock(), "links::parked").clone()
}

fn emit_drafts(app: &AppHandle, drafts: Vec<Option<String>>) {
    for name in drafts {
        let _ = emit_event_to_main(app, WritFrontendEvent::LinkNew { name });
    }
}
