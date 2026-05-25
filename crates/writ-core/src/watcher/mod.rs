//! External-change events and conflict-resolution policy.
//!
//! The watcher module is pure policy: the actual filesystem watcher is
//! implemented in `writ-tauri`. The types here describe *what* a change
//! looks like and *how* it should be resolved, independent of how the
//! change was observed.

/// Typed representation of externally-observed file changes.
pub mod change_event;
/// Conflict-resolution policy for external changes.
pub mod conflict;
/// Content-fingerprinted ignore stamps for distinguishing internal writes
/// from real external edits, even when a debouncer coalesces both into a
/// single delivered event.
pub mod ignore;
