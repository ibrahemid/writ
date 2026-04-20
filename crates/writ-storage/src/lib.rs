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
//! - [`database`]: raw connection management, migrations, and query
//!   primitives.
//! - [`buffer_store`]: high-level buffer CRUD on top of `database`.
//! - [`config_store`]: TOML config load and save.
//! - [`consistency`]: startup checks that reconcile the database with
//!   the on-disk buffer directory.
//! - [`fts`]: FTS5 indexing and search over buffer content.
//! - [`recovery`]: session snapshots and dirty-shutdown detection.
//! - [`errors`]: crate-wide [`errors::StorageError`] /
//!   [`errors::StorageResult`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// High-level buffer CRUD built on top of [`database`].
pub mod buffer_store;
/// TOML configuration load and save.
pub mod config_store;
/// Startup consistency checker reconciling database and disk.
pub mod consistency;
/// Raw connection management, migrations, and query primitives.
pub mod database;
/// Crate-wide error and result types.
pub mod errors;
/// FTS5 indexing and search over buffer content.
pub mod fts;
/// Session snapshots and dirty-shutdown detection.
pub mod recovery;
