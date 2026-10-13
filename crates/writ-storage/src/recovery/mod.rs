//! The unsaved-text journal, session markers and dirty-shutdown detection
//! (ADR-044).
//!
//! This module is wired into the running app. The flow, end to end:
//!
//! 1. **Journal**: `src-tauri/src/commands/buffer.rs` writes the text of a
//!    tab whose save did not land into [`journal::UnsavedJournal`] as the save
//!    command returns, as that tab's entry, and the next save of that tab that
//!    lands clears it. The quit and the closing of a tab hand over what the
//!    editor still holds the same way (`record_unsaved_notes`). Reopening a
//!    closed note releases its entry from the tab (`BufferStore::restore`), so
//!    no save of the new tab, which shows the file, clears that text.
//! 2. **Restore**: `src-tauri/src/state.rs` reads the journal on every
//!    launch, after [`legacy::carry_over_legacy_snapshots`] has moved in
//!    whatever a 0.5 snapshot was holding, and releases every entry first.
//!    Each entry is written through `BufferStore::restore_recovered_content`
//!    and removed once it is in a file; one whose file was deleted outside
//!    Writ stays, and its tab is handed the newest such text. A row that does
//!    not decode is logged and kept.
//! 3. **Pull surface**: the frontend asks for what the launch restored
//!    through the `get_recovered_buffers` IPC command (`src/App.tsx` on boot)
//!    and shows a toast. Recovery is pull-based by design: the frontend reads
//!    the list when it is ready, which is deterministic and carries the buffer
//!    detail a fire-and-forget startup event cannot.
//! 4. **Markers**: `src-tauri/src/heartbeat.rs` records an unclean marker
//!    once per session, at the first beat of a heartbeat that runs every
//!    120 s (`heartbeat::SNAPSHOT_HEARTBEAT`), and the quit records a clean
//!    one through [`snapshot::SnapshotManager`]. Neither reads a file or
//!    carries text. [`dirty_shutdown::check_dirty_shutdown`] reads the latest
//!    marker for the launch's log; nothing is restored on its account.
//! 5. **Consistency**: `state.rs` runs
//!    [`crate::consistency::ConsistencyChecker`] on the same boot path and
//!    logs orphan / missing backing files (repair policy is a separate ADR).

/// Dirty-shutdown detection driven by the latest marker's `clean` flag.
pub mod dirty_shutdown;
/// The unsaved-text journal.
pub mod journal;
/// The one-time move of a 0.5 snapshot's text into the journal.
pub mod legacy;
/// Session markers.
pub mod snapshot;
