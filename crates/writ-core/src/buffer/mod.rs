//! Buffer domain model.
//!
//! A *buffer* is Writ's unit of editable content. It has a lifecycle
//! (active as a tab, then moved to history on close), a stable id, and
//! optional linkage to a source file on disk.

/// Edits in CodeMirror's form, applied to the text they were made against.
pub mod change_set;
/// Document-level buffer model.
pub mod document;
/// The language chosen for a buffer.
pub mod language;
/// In-memory buffer manager and lifecycle operations.
pub mod manager;

pub use document::{BufferDocument, BufferStatus};
pub use manager::BufferManager;
