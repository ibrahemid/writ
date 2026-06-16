//! Session snapshots and dirty-shutdown detection.
//!
//! This module is wired into the running app. The flow, end to end:
//!
//! 1. **Boot detection** — `src-tauri/src/state.rs` calls
//!    [`crate::buffer_store::BufferStore::is_dirty_shutdown`] (backed by
//!    [`dirty_shutdown::check_dirty_shutdown`]) right after migrations. On a
//!    dirty shutdown it resolves the recovered buffers via
//!    `BufferStore::resolve_recovery`, writes them back to disk, and stashes
//!    them on `AppState`.
//! 2. **Pull surface** — the frontend asks for the recovered set through the
//!    `get_recovered_buffers` IPC command (`src/App.tsx` on boot) and shows a
//!    toast. Recovery is pull-based by design: the frontend reads the list
//!    when it is ready, which is deterministic and carries the buffer detail a
//!    fire-and-forget startup event cannot.
//! 3. **Snapshots** — `src-tauri/src/lib.rs` writes an unclean heartbeat
//!    snapshot every 30 s and a clean snapshot on `ExitRequested`, via
//!    [`snapshot::SnapshotManager`] / `BufferStore::write_session_snapshot`.
//!    The `is_clean` flag is the contract `check_dirty_shutdown` reads.
//! 4. **Consistency** — `state.rs` runs
//!    [`crate::consistency::ConsistencyChecker`] on the same boot path and
//!    logs orphan / missing backing files (repair policy is a separate ADR).

/// Dirty-shutdown detection driven by the latest snapshot's `clean` flag.
pub mod dirty_shutdown;
/// Session snapshot storage and retrieval.
pub mod snapshot;
