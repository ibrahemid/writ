//! Persistence layer for the Writ editor.
//!
//! `writ-storage` owns every disk interaction Writ performs: the SQLite
//! database, on-disk buffer content files, TOML configuration, session
//! snapshots, and the FTS5 search index. It depends on `writ-core` for
//! domain types and exposes higher-level stores that the Tauri adapter
//! composes into IPC commands.
//!
//! # Module layout
//!
//! - [`atomic`]: temp-file + fsync + rename helper for crash-safe writes.
//! - [`database`]: raw connection management, migrations, and query
//!   primitives.
//! - [`buffer_store`]: high-level buffer CRUD on top of `database`.
//! - [`config_store`]: TOML config load and save.
//! - [`consistency`]: startup checks that reconcile the database with the
//!   files the notes live in.
//! - [`fts`]: FTS5 indexing and search over buffer content.
//! - [`maintenance`]: WAL checkpointing and freelist reclamation.
//! - [`notes_migration`]: the one-time pass that turns every note into a
//!   file.
//! - [`recovery`]: session snapshots and dirty-shutdown detection.
//! - [`rollback`]: the copy of the database taken before the notes
//!   migration writes anything.
//! - [`schema_meta`]: key/value rows recording what a one-time schema pass
//!   did.
//! - [`errors`]: crate-wide [`errors::StorageError`] /
//!   [`errors::StorageResult`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Crash-safe file writes via temp-file + fsync + rename.
pub mod atomic;
/// High-level buffer CRUD built on top of [`database`].
pub mod buffer_store;
/// TOML configuration load and save.
pub mod config_store;
/// Startup consistency checker reconciling the database with the files.
pub mod consistency;
/// Raw connection management, migrations, and query primitives.
pub mod database;
/// Crate-wide error and result types.
pub mod errors;
/// FTS5 indexing and search over buffer content.
pub mod fts;
/// Watched-inbox file listing.
pub mod inbox_store;
/// Per-buffer preview layout persistence (ADR-009).
pub mod layout_state;
/// WAL checkpointing and freelist reclamation.
pub mod maintenance;
/// The one-time pass that turns every note into a file (ADR-028).
pub mod notes_migration;
/// Session snapshots and dirty-shutdown detection.
pub mod recovery;
/// The copy of the database taken before the notes migration (ADR-028).
pub mod rollback;
/// Key/value rows recording what a one-time schema pass did.
pub mod schema_meta;
/// On-demand content grep over the workspace folder (ADR-026).
pub mod workspace_grep;
/// Workspace file-name index walk and the shared search ignore policy.
pub mod workspace_search;
/// Workspace directory listing with traversal safety.
pub mod workspace_store;
