//! Buffer domain model.
//!
//! A *buffer* is Writ's unit of editable content. It has a lifecycle
//! (active as a tab, then moved to history on close), a stable id, and
//! optional linkage to a source file on disk.

/// Document-level buffer model.
pub mod document;
/// In-memory buffer manager and lifecycle operations.
pub mod manager;

pub use document::{BufferDocument, BufferStatus};
pub use manager::BufferManager;
