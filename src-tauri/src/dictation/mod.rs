//! Dictation's mechanism: the on-device bridge ([`bridge`]), the session
//! driver ([`session`]), what the main window ends ([`window`]) and the one
//! live session the app allows.
//!
//! What a session reports and when lives in `writ_core::dictation`; the IPC
//! surface is `commands::dictation`.

pub mod bridge;
pub mod session;
pub mod window;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use writ_core::dictation::DictationError;

use crate::poison::recover_poison;
use session::Controls;

struct Live {
    id: u32,
    controls: Controls,
}

/// The live session, if any. One session at a time: a second start while one
/// runs is refused as busy.
pub struct DictationState {
    live: Mutex<Option<Live>>,
    next_id: AtomicU32,
}

impl Default for DictationState {
    fn default() -> Self {
        Self {
            live: Mutex::new(None),
            next_id: AtomicU32::new(1),
        }
    }
}

impl DictationState {
    /// Takes the session slot. Dropping the reservation frees it again unless
    /// it was kept for a session that started ([`Reservation::keep`]).
    pub fn reserve(&self) -> Result<Reservation<'_>, DictationError> {
        let mut live = recover_poison(self.live.lock(), "dictation::reserve");
        if live.is_some() {
            return Err(DictationError::Busy);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let controls = Controls::default();
        *live = Some(Live {
            id,
            controls: controls.clone(),
        });
        Ok(Reservation {
            state: self,
            id,
            controls,
            kept: false,
        })
    }

    /// Frees the slot held by session `id`; another session's slot is left.
    pub fn release(&self, id: u32) {
        let mut live = recover_poison(self.live.lock(), "dictation::release");
        if live.as_ref().is_some_and(|l| l.id == id) {
            *live = None;
        }
    }

    /// Asks session `id` to stop: capture ends and what was heard is still
    /// inserted. An id that is not live has already ended.
    pub fn stop(&self, id: u32) {
        if let Some(controls) = self.controls(id) {
            controls.stop.cancel();
        }
    }

    /// Cancels session `id`: what is not inserted yet is discarded. An id that
    /// is not live has already ended.
    pub fn cancel(&self, id: u32) {
        if let Some(controls) = self.controls(id) {
            controls.cancel.cancel();
        }
    }

    /// Asks whatever session holds the slot, a start still being set up
    /// included, to stop. Returns its id.
    pub fn stop_live(&self) -> Option<u32> {
        let (id, controls) = self.live_controls()?;
        controls.stop.cancel();
        Some(id)
    }

    /// Cancels whatever session holds the slot, a start still being set up
    /// included. Returns its id.
    pub fn cancel_live(&self) -> Option<u32> {
        let (id, controls) = self.live_controls()?;
        controls.cancel.cancel();
        Some(id)
    }

    /// Whether a session holds the slot.
    pub fn is_busy(&self) -> bool {
        recover_poison(self.live.lock(), "dictation::is_busy").is_some()
    }

    fn controls(&self, id: u32) -> Option<Controls> {
        let live = recover_poison(self.live.lock(), "dictation::controls");
        live.as_ref()
            .filter(|l| l.id == id)
            .map(|l| l.controls.clone())
    }

    fn live_controls(&self) -> Option<(u32, Controls)> {
        let live = recover_poison(self.live.lock(), "dictation::live_controls");
        live.as_ref().map(|l| (l.id, l.controls.clone()))
    }
}

/// Runs `release` when dropped, so a session task that unwinds still frees
/// its slot.
pub struct ReleaseOnDrop<F: FnOnce()> {
    release: Option<F>,
}

impl<F: FnOnce()> ReleaseOnDrop<F> {
    /// A guard that runs `release` once, when it is dropped.
    pub fn new(release: F) -> Self {
        Self {
            release: Some(release),
        }
    }
}

impl<F: FnOnce()> Drop for ReleaseOnDrop<F> {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
    }
}

/// The session slot, held while a start is checked and set up.
pub struct Reservation<'a> {
    state: &'a DictationState,
    id: u32,
    controls: Controls,
    kept: bool,
}

impl Reservation<'_> {
    /// The session's id.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The tokens that stop and cancel the session.
    pub fn controls(&self) -> Controls {
        self.controls.clone()
    }

    /// Keeps the slot for a session that started; the session releases it
    /// when it ends.
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.kept {
            self.state.release(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_start_while_one_runs_is_busy() {
        let state = DictationState::default();
        let first = state.reserve().expect("the slot is free");
        assert!(matches!(state.reserve(), Err(DictationError::Busy)));
        first.keep();
        assert!(matches!(state.reserve(), Err(DictationError::Busy)));
    }

    #[test]
    fn a_start_that_fails_frees_the_slot() {
        let state = DictationState::default();
        let id = {
            let reservation = state.reserve().expect("the slot is free");
            reservation.id()
        };
        let next = state.reserve().expect("the failed start freed the slot");
        assert_ne!(next.id(), id);
    }

    #[test]
    fn a_session_that_ends_frees_the_slot() {
        let state = DictationState::default();
        let reservation = state.reserve().expect("the slot is free");
        let id = reservation.id();
        reservation.keep();
        assert!(state.is_busy());
        state.release(id + 1);
        assert!(state.is_busy(), "another id's release leaves the slot");
        state.release(id);
        assert!(!state.is_busy());
    }

    #[test]
    fn stop_and_cancel_reach_the_live_session_only() {
        let state = DictationState::default();
        let reservation = state.reserve().expect("the slot is free");
        let id = reservation.id();
        let controls = reservation.controls();
        reservation.keep();

        state.stop(id + 7);
        state.cancel(id + 7);
        assert!(!controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled());

        state.stop(id);
        assert!(controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled());
        state.cancel(id);
        assert!(controls.cancel.is_cancelled());

        state.release(id);
        state.stop(id);
        state.cancel(id);
    }

    #[test]
    fn stop_live_reaches_a_start_still_being_set_up() {
        let state = DictationState::default();
        assert_eq!(state.stop_live(), None);
        let reservation = state.reserve().expect("the slot is free");
        let controls = reservation.controls();

        assert_eq!(state.stop_live(), Some(reservation.id()));
        assert!(controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled());
    }

    #[test]
    fn cancel_live_reaches_the_running_session() {
        let state = DictationState::default();
        assert_eq!(state.cancel_live(), None);
        let reservation = state.reserve().expect("the slot is free");
        let id = reservation.id();
        let controls = reservation.controls();
        reservation.keep();

        assert_eq!(state.cancel_live(), Some(id));
        assert!(controls.cancel.is_cancelled());
        state.release(id);
        assert_eq!(state.cancel_live(), None);
    }

    #[test]
    fn a_session_task_that_panics_frees_the_slot() {
        let state = std::sync::Arc::new(DictationState::default());
        let reservation = state.reserve().expect("the slot is free");
        let id = reservation.id();
        reservation.keep();

        let task_state = std::sync::Arc::clone(&state);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _slot = ReleaseOnDrop::new(move || task_state.release(id));
            panic!("the session driver failed");
        }));

        assert!(unwound.is_err());
        assert!(!state.is_busy());
        assert!(state.reserve().is_ok());
    }

    #[test]
    fn the_guard_releases_once_on_a_normal_end() {
        let calls = std::sync::atomic::AtomicU32::new(0);
        drop(ReleaseOnDrop::new(|| {
            calls.fetch_add(1, Ordering::Relaxed);
        }));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
