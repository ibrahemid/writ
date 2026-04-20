//! Pure domain logic for the Writ editor.
//!
//! `writ-core` is the policy layer of Writ: it defines the buffer model,
//! configuration schema, conflict resolution, command registry, history,
//! and event payloads. It has no knowledge of Tauri, SQLite, the
//! filesystem, or any specific runtime.
//!
//! # Crate boundary
//!
//! This crate is intentionally framework-free. It depends only on `serde`,
//! `tracing`, and small utility crates. Persistence lives in the sibling
//! `writ-storage` crate, and the IPC adapter lives in `writ-tauri`.
//!
//! # Module layout
//!
//! - [`buffer`]: document model and the in-memory [`buffer::BufferManager`].
//! - [`command`]: command registry types for palette-driven actions.
//! - [`config`]: typed user configuration with serde defaults.
//! - [`errors`]: crate-wide [`errors::WritError`] / [`errors::WritResult`].
//! - [`events`]: strongly-typed domain events and an in-process event bus.
//! - [`file_ops`]: pure helpers for file validation and language detection.
//! - [`history`]: recently-closed buffer history.
//! - [`watcher`]: external-change representation and conflict policy.
//! - [`workspace`]: workspace-level state (reserved).

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Buffer document model and in-memory manager.
pub mod buffer;
/// Command registry types for the command palette.
pub mod command;
/// Typed configuration schema with serde defaults.
pub mod config;
/// Crate-wide error and result types.
pub mod errors;
/// Strongly-typed domain events and an in-process event bus.
pub mod events;
/// Pure file helpers: validation, language detection, filename extraction.
pub mod file_ops;
/// Recently-closed buffer history types.
pub mod history;
/// External-change events and conflict-resolution policy.
pub mod watcher;
/// Workspace-level state (reserved for future expansion).
pub mod workspace;
