//! Notes-folder configuration (`[notes]`).

use serde::{Deserialize, Serialize};

fn default_notes_root() -> Option<String> {
    None
}

/// Notes-folder configuration (`[notes]`).
///
/// `None` is the shipped state and means the default folder, resolved by
/// [`crate::notes::resolve_notes_root`]. Writing the resolved path into the
/// config on first launch would make the default look like a choice the user
/// made, and moving the folder later would then have to rewrite it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotesConfig {
    /// Absolute path to the notes folder, or `None` for the default.
    #[serde(default = "default_notes_root")]
    pub root: Option<String>,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            root: default_notes_root(),
        }
    }
}
