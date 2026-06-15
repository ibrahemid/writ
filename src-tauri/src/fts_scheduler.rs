//! Coalesced, deferred FTS reindex scheduling (ADR-020).
//!
//! The autosave IPC path writes buffer content immediately but defers the
//! FTS reindex so the index cost leaves the keystroke loop. This scheduler
//! owns the timing policy: per buffer it keeps a monotonically increasing
//! edit generation and at most one worker. A worker waits a debounce window,
//! and reindexes only once the generation stops advancing — a trailing
//! debounce that collapses an edit burst into a single reindex of the latest
//! on-disk content.
//!
//! Timing (the sleep) lives in the command that spawns the worker; this
//! module is pure state so its coalescing decisions are unit-testable without
//! a runtime.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

/// Debounce window between the last edit and the deferred reindex.
pub const FTS_REINDEX_DEBOUNCE: Duration = Duration::from_secs(2);

/// Outcome of a worker poll after one debounce window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    /// The buffer has settled at the seen generation; reindex now and stop.
    Reindex,
    /// A newer edit arrived; wait another window against this generation.
    Wait(u64),
}

#[derive(Default)]
struct SchedulerInner {
    generation: HashMap<String, u64>,
    has_worker: HashSet<String>,
    pending: HashSet<String>,
}

/// Per-buffer reindex coalescer. Cheap to clone-free share behind `AppState`.
#[derive(Default)]
pub struct FtsScheduler {
    inner: Mutex<SchedulerInner>,
}

impl FtsScheduler {
    /// Constructs an empty scheduler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an edit to `id`. Bumps the buffer's generation and marks it
    /// pending. Returns `Some(generation)` when the caller should spawn a
    /// worker (none is currently running for this buffer); returns `None`
    /// when a worker already owns this buffer and will observe the bump.
    pub fn on_edit(&self, id: &str) -> Option<u64> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let generation = {
            let g = inner.generation.entry(id.to_string()).or_insert(0);
            *g += 1;
            *g
        };
        inner.pending.insert(id.to_string());
        if inner.has_worker.contains(id) {
            None
        } else {
            inner.has_worker.insert(id.to_string());
            Some(generation)
        }
    }

    /// Called by a worker after one debounce window. If the buffer's
    /// generation has not advanced past `seen`, the buffer has settled:
    /// the worker is retired and the buffer cleared from pending, and the
    /// caller should reindex. Otherwise the worker should wait another window
    /// against the returned generation.
    pub fn poll(&self, id: &str, seen: u64) -> PollOutcome {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let current = inner.generation.get(id).copied().unwrap_or(0);
        if current == seen {
            // Settled: retire the worker and forget the buffer entirely so the
            // maps stay bounded over a long session. A later edit re-inserts at
            // generation 1 and spawns a fresh worker (a missing entry reads as
            // 0, so `on_edit` bumps to 1).
            inner.has_worker.remove(id);
            inner.pending.remove(id);
            inner.generation.remove(id);
            PollOutcome::Reindex
        } else {
            PollOutcome::Wait(current)
        }
    }

    /// Drains every buffer still awaiting a reindex, retiring all workers.
    /// Used on shutdown to flush deferred reindexes synchronously so a quick
    /// quit cannot leave the index trailing the on-disk content.
    pub fn drain_pending(&self) -> Vec<String> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.has_worker.clear();
        inner.pending.drain().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_edit_spawns_a_worker_subsequent_edits_do_not() {
        let s = FtsScheduler::new();
        assert_eq!(s.on_edit("buf"), Some(1), "first edit must spawn a worker");
        assert_eq!(s.on_edit("buf"), None, "worker already running");
        assert_eq!(s.on_edit("buf"), None);
    }

    #[test]
    fn settled_buffer_reindexes_and_retires_the_worker() {
        let s = FtsScheduler::new();
        let gen = s.on_edit("buf").expect("worker");
        // No further edits: the worker sees the same generation and reindexes.
        assert_eq!(s.poll("buf", gen), PollOutcome::Reindex);
        // Worker retired and the settled buffer forgotten, so the next edit
        // spawns a fresh worker starting again at generation 1.
        assert_eq!(s.on_edit("buf"), Some(1));
    }

    #[test]
    fn edits_during_the_window_make_the_worker_wait() {
        let s = FtsScheduler::new();
        let gen = s.on_edit("buf").expect("worker");
        // Two more edits land before the window elapses.
        assert_eq!(s.on_edit("buf"), None);
        assert_eq!(s.on_edit("buf"), None);
        // The worker polls against its stale generation and is told to wait.
        match s.poll("buf", gen) {
            PollOutcome::Wait(g) => assert_eq!(g, 3),
            other => panic!("expected Wait, got {other:?}"),
        }
        // Settling at the latest generation finally reindexes.
        assert_eq!(s.poll("buf", 3), PollOutcome::Reindex);
    }

    #[test]
    fn distinct_buffers_get_independent_workers() {
        let s = FtsScheduler::new();
        assert_eq!(s.on_edit("a"), Some(1));
        assert_eq!(s.on_edit("b"), Some(1));
        assert_eq!(s.poll("a", 1), PollOutcome::Reindex);
        // b is unaffected by a settling.
        assert_eq!(s.poll("b", 1), PollOutcome::Reindex);
    }

    #[test]
    fn drain_pending_returns_unflushed_buffers_and_retires_workers() {
        let s = FtsScheduler::new();
        s.on_edit("a");
        s.on_edit("b");
        s.on_edit("a"); // still one pending entry for a
        let mut drained = s.drain_pending();
        drained.sort();
        assert_eq!(drained, vec!["a".to_string(), "b".to_string()]);
        // After a drain the workers are retired, so a new edit spawns again.
        // The generation counter stays monotonic across drains (a is at 2, so
        // the next edit is 3); only the worker/pending state resets.
        assert_eq!(s.on_edit("a"), Some(3));
    }

    #[test]
    fn settled_then_drained_is_empty() {
        let s = FtsScheduler::new();
        let gen = s.on_edit("a").expect("worker");
        assert_eq!(s.poll("a", gen), PollOutcome::Reindex);
        // The settled buffer was cleared from pending, so nothing to flush.
        assert!(s.drain_pending().is_empty());
    }
}
