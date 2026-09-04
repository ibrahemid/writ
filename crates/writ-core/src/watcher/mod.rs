//! External-change events.
//!
//! The watcher module is pure policy: the actual filesystem watcher is
//! implemented in `writ-tauri`. The types here describe *what* a change looks
//! like, independent of how the change was observed. What a save may do when
//! the file changed underneath it is [`crate::notes::guard`].

/// Typed representation of externally-observed file changes.
pub mod change_event;
/// Content-fingerprinted ignore stamps for distinguishing internal writes
/// from real external edits, even when a debouncer coalesces both into a
/// single delivered event.
pub mod ignore;
