//! The shutdown a restart or an update install runs before the process goes.
//!
//! Neither passes through an exit request the quit handshake can hold:
//! `AppHandle::restart` on the main thread relaunches without raising
//! `ExitRequested`, and on Windows the updater plugin calls
//! `std::process::exit` after starting the installer. So both run the
//! handshake first: ask the window to flush, wait for its answer, then write
//! what Rust holds ([`crate::finish_shutdown`]).
//!
//! The work is claimed through [`crate::quit::QuitState`], the same phase a
//! quit claims, so whichever path starts first does it once and the others
//! find it done.

use std::sync::atomic::Ordering;
use std::time::Duration;

use writ_core::events::bus::WritEvent;

use crate::quit::QuitDecision;
use crate::state::AppState;

/// How long a relaunch waits for a quit that is already writing.
///
/// That quit waits for the window up to
/// [`writ_core::recovery::QUIT_FLUSH_TIMEOUT`], then stops chat replies,
/// drains deferred reindexes and writes the snapshot; one still running after
/// this is stuck.
const QUIT_IN_FLIGHT_LIMIT: Duration = Duration::from_secs(8);

/// What a relaunch may do once [`shut_down_for_relaunch`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelaunchShutdown {
    /// This call flushed the window and ran the shutdown work, so the restart
    /// or the installer may go ahead.
    Finished,
    /// A quit or an earlier relaunch had already claimed the work, and it ends
    /// the process itself. A restart asks for nothing more; an installer that
    /// is about to exit has waited for that work to finish.
    AlreadyLeaving,
}

/// Runs the quit's handshake and the shutdown work before a relaunch.
///
/// Blocks for as long as the window takes to answer, bounded by
/// [`writ_core::recovery::QUIT_FLUSH_TIMEOUT`], so it must not run on the main
/// thread: the window's answer is a command handled there.
///
/// `finish_shutdown` is the work itself, passed in because it needs the
/// running app's handle ([`crate::finish_shutdown`]).
pub fn shut_down_for_relaunch(
    state: &AppState,
    finish_shutdown: impl FnOnce(),
) -> RelaunchShutdown {
    match state.quit.begin(None) {
        QuitDecision::StartFlush => {}
        QuitDecision::Wait => {
            if !state.quit.wait_until_complete(QUIT_IN_FLIGHT_LIMIT) {
                tracing::warn!("the quit already writing did not finish before the relaunch");
            }
            return RelaunchShutdown::AlreadyLeaving;
        }
        QuitDecision::Proceed => return RelaunchShutdown::AlreadyLeaving,
    }

    // A cancelled reconcile walk removes nothing, and the next launch walks
    // again; waiting for it would hold the relaunch for the whole folder.
    state.notes_index_cancel.store(true, Ordering::Relaxed);

    if state.frontend_ready.load(Ordering::SeqCst) {
        state.event_bus.emit(WritEvent::FlushBeforeQuit);
        if !state.quit.wait_for_flush() {
            tracing::warn!("the window did not confirm its flush before the relaunch");
        }
    }

    finish_shutdown();
    state.quit.finish();
    RelaunchShutdown::Finished
}
