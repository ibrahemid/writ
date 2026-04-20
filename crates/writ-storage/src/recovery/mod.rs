//! Session snapshots and dirty-shutdown detection.
//!
//! Writ records a session snapshot on clean shutdown; the presence of a
//! snapshot without a `clean` marker indicates the previous run crashed.
//! Callers use this to offer recovery on relaunch.

/// Dirty-shutdown detection driven by the latest snapshot's `clean` flag.
pub mod dirty_shutdown;
/// Session snapshot storage and retrieval.
pub mod snapshot;
