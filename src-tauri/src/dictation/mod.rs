//! Dictation's mechanism: the on-device bridge ([`bridge`]), the session
//! driver ([`session`]) and the one live session the app allows.
//!
//! What a session reports and when lives in `writ_core::dictation`; the IPC
//! surface is `commands::dictation`.

pub mod bridge;
pub mod session;

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
}
