//! Session snapshots and dirty-shutdown detection.
//!
//! # STATUS: UNWIRED INFRASTRUCTURE — NOT CALLED BY THE RUNNING APP.
//!
//! This module ships as reusable scaffolding for a future crash-recovery
//! feature. The database schema, snapshot writes, and dirty-shutdown
//! detection are implemented and tested in isolation, but the Tauri shell
//! (`src-tauri`) does not call any of these functions during startup or
//! shutdown. No `RecoveryDirty` event is ever emitted, and no UI surface
//! consumes one. Treat any public documentation that implies "crash
//! recovery on relaunch" as aspirational until this wiring lands.
//!
//! # To resurrect, in order:
//!
//! 1. In `src-tauri/src/state.rs`, after [`crate::database::migrations::run_migrations`]
//!    and **before** [`crate::buffer_store::BufferStore::new`] consumes the
//!    connection, call [`dirty_shutdown::check_dirty_shutdown`] and stash
//!    the result (plus the latest snapshot id and buffer count) on
//!    `AppState`.
//! 2. In `src-tauri/src/lib.rs` `setup`, if the stashed flag is set, emit
//!    `WritFrontendEvent::RecoveryDirty` via the existing `emit_event`.
//! 3. Add a frontend listener for the `recovery:dirty` channel in
//!    `src/services/events.ts` consumers (e.g. `src/App.tsx`) that
//!    surfaces a toast or restore prompt.
//! 4. Decide what session state to snapshot, then write it via
//!    [`snapshot::SnapshotManager::write_snapshot`] at well-defined
//!    points (periodic timer, clean-shutdown hook on
//!    `WindowEvent::CloseRequested`). The current `is_clean` flag is the
//!    contract `check_dirty_shutdown` relies on.
//! 5. Wire [`crate::consistency::ConsistencyChecker`] into the same boot
//!    path to repair orphaned / missing buffer files.
//!
//! Until the wiring above exists, every user-facing surface (README,
//! CHANGELOG, ARCHITECTURE, site) must describe this module as
//! "infrastructure only, not yet wired." A test in
//! `crates/writ-storage/tests/recovery_unwired_contract.rs` enforces the
//! UNWIRED marker in this docstring so the lie cannot quietly return.

/// Dirty-shutdown detection driven by the latest snapshot's `clean` flag.
pub mod dirty_shutdown;
/// Session snapshot storage and retrieval.
pub mod snapshot;
