use serde::{Deserialize, Serialize};

/// Nature of a change observed on a buffer's backing file from outside
/// Writ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExternalChange {
    /// The file's contents were modified.
    Modified,
    /// The file was deleted from disk.
    Deleted,
}
