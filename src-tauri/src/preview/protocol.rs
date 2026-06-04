//! `writ-preview://` protocol — disposition recorder.
//!
//! The pure URL parser ([`parse`], [`ParsedRequest`], [`PreviewScope`],
//! [`RefusalReason`]) lives in `writ-core` (`writ_core::preview::protocol`)
//! so it carries no Tauri dependency and can be fuzzed without the app
//! shell. It is re-exported here so the handler and tests keep importing
//! from `super::protocol`.
//!
//! This module owns the debug-only request recorder: a thread-local log of
//! every disposition the handler reaches, read by the scope/traversal half
//! of the verification suite (`tests/preview_security.rs`). It is compiled
//! out of release builds.

use std::cell::RefCell;

pub use writ_core::preview::protocol::{parse, ParsedRequest, PreviewScope, RefusalReason};

/// Disposition of a single request, as recorded for diagnostics and for
/// the verification suite's assertion hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// Request was permitted and served.
    Allowed,
    /// Request was rejected by the protocol handler before any I/O.
    Refused(RefusalReason),
}

/// Single entry in the debug-only request log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRecord {
    /// The full requested URL.
    pub url: String,
    /// Decision made by the handler.
    pub disposition: Disposition,
}

#[cfg(any(test, debug_assertions))]
thread_local! {
    static RECORDER: RefCell<Vec<RequestRecord>> = const { RefCell::new(Vec::new()) };
}

/// Record a disposition for the current thread. No-op in release builds.
#[cfg(any(test, debug_assertions))]
pub fn record(record: RequestRecord) {
    RECORDER.with(|r| r.borrow_mut().push(record));
}

/// Record stub: release builds compile this away entirely.
#[cfg(not(any(test, debug_assertions)))]
pub fn record(_record: RequestRecord) {}

/// Drain the recorded requests for the current thread. Always returns an
/// empty vec in release builds.
#[cfg(any(test, debug_assertions))]
pub fn drain_records() -> Vec<RequestRecord> {
    RECORDER.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

#[cfg(not(any(test, debug_assertions)))]
pub fn drain_records() -> Vec<RequestRecord> {
    Vec::new()
}

/// Clear the recorder without returning its contents.
#[cfg(any(test, debug_assertions))]
pub fn clear_records() {
    RECORDER.with(|r| r.borrow_mut().clear());
}

#[cfg(not(any(test, debug_assertions)))]
pub fn clear_records() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_round_trip() {
        clear_records();
        record(RequestRecord {
            url: "writ-preview://chrome/x".to_string(),
            disposition: Disposition::Allowed,
        });
        record(RequestRecord {
            url: "writ-preview://document/../x".to_string(),
            disposition: Disposition::Refused(RefusalReason::TraversalAttempt),
        });
        let drained = drain_records();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].disposition, Disposition::Allowed);
        assert!(matches!(
            drained[1].disposition,
            Disposition::Refused(RefusalReason::TraversalAttempt)
        ));
        // Drained: subsequent drain is empty.
        assert!(drain_records().is_empty());
    }
}
