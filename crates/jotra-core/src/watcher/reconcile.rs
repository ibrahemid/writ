//! Keeping one folder walk running at a time without losing the requests
//! that arrive while it does.
//!
//! Walking the notes folder reads every note in it, so two walks over one
//! folder read everything twice for one answer. Only one runs, and a request
//! that arrives while it does is remembered rather than started: whatever it
//! was about either lands in the walk already running, or in exactly one walk
//! after it.
//!
//! Policy only: no thread, no clock, no filesystem. The caller runs the walk
//! and reports back.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a walk may start, and whether one is owed when the current walk
/// ends.
///
/// The pair of flags is what closes the hole a single "one at a time" flag
/// leaves. A request that lands while a walk is running used to be dropped
/// outright, on the reasoning that the walk reads the folder as it finds it.
/// That is true only for changes the walk has not reached yet: a file rewritten
/// behind the walk's back was read before it changed, and nothing was left to
/// go and read it again.
#[derive(Debug, Default)]
pub struct ReconcileGate {
    walking: AtomicBool,
    owed: AtomicBool,
}

impl ReconcileGate {
    /// A gate with nothing running and nothing owed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks for a walk. `true` when the caller should start one now; `false`
    /// when one is already running, in which case it is now owed a successor.
    pub fn request(&self) -> bool {
        if self.walking.swap(true, Ordering::SeqCst) {
            self.owed.store(true, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Reports a walk finished. `true` when a request arrived while it ran and
    /// the caller should walk once more; `false` when the gate is now open.
    ///
    /// The second look at `owed` catches a request that landed between the
    /// first look and the gate opening: it saw a walk running, so it started
    /// nothing, and the debt it left would otherwise be paid by nobody. If
    /// something else has taken the gate by then, that walk starts after the
    /// request and covers it.
    pub fn finished(&self) -> bool {
        if self.owed.swap(false, Ordering::SeqCst) {
            return true;
        }
        self.walking.store(false, Ordering::SeqCst);
        self.owed.swap(false, Ordering::SeqCst) && !self.walking.swap(true, Ordering::SeqCst)
    }

    /// Gives the gate back without walking, and forgets anything owed.
    ///
    /// Shutdown, where a walk asked for during the last one is not a reason to
    /// keep reading the folder. Everywhere else, end a walk with
    /// [`Self::finished`].
    pub fn release(&self) {
        self.owed.store(false, Ordering::SeqCst);
        self.walking.store(false, Ordering::SeqCst);
    }

    /// Whether a walk is running. For logs and tests; a caller deciding
    /// anything on this would be racing it.
    pub fn is_walking(&self) -> bool {
        self.walking.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_request_walks() {
        let gate = ReconcileGate::new();
        assert!(gate.request());
        assert!(gate.is_walking());
    }

    #[test]
    fn a_request_while_a_walk_runs_starts_nothing() {
        let gate = ReconcileGate::new();
        assert!(gate.request());
        assert!(!gate.request());
        assert!(!gate.request());
    }

    #[test]
    fn a_burst_during_one_walk_costs_exactly_one_more() {
        // The storm case: a sweep marker per cooldown for as long as a sync
        // client runs, every one of them landing on a walk already going.
        let gate = ReconcileGate::new();
        assert!(gate.request());
        for _ in 0..50 {
            assert!(!gate.request());
        }

        assert!(gate.finished(), "the walk owes the burst one more");
        assert!(!gate.finished(), "and only one");
        assert!(!gate.is_walking());
    }

    #[test]
    fn a_walk_nothing_interrupted_leaves_the_gate_open() {
        let gate = ReconcileGate::new();
        assert!(gate.request());
        assert!(!gate.finished());
        assert!(!gate.is_walking());
        assert!(gate.request(), "the next request walks again");
    }

    #[test]
    fn a_gate_given_back_owes_nothing_and_lets_the_next_walk_in() {
        let gate = ReconcileGate::new();
        assert!(gate.request());
        assert!(!gate.request());

        gate.release();

        assert!(!gate.is_walking());
        assert!(gate.request(), "the gate was not given back");
        assert!(!gate.finished(), "the abandoned walk is still owed");
    }

    #[test]
    fn a_request_between_two_walks_is_not_swallowed_by_the_first() {
        let gate = ReconcileGate::new();
        assert!(gate.request());
        assert!(!gate.request());
        assert!(gate.finished());

        // The follow-up is running now, and what arrives during it is owed
        // the same way: nothing is special about being a follow-up.
        assert!(!gate.request());
        assert!(gate.finished());
        assert!(!gate.finished());
    }
}
