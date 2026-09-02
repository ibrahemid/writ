use serde::{Deserialize, Serialize};

/// Nature of a change observed on a buffer's backing file from outside
/// Writ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExternalChange {
    /// The file's contents were modified.
    Modified,
    /// The file is gone and nothing carrying its identity was found, so the
    /// tab keeps its text and stops writing to the path (spec W4).
    Removed,
    /// The same file is at another path now. The tab follows it there; the
    /// text is untouched, because a move changes no bytes.
    Moved,
}
