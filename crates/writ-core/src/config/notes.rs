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

/// Why a moved folder's path could not be put into a config file's text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotesRootEditError {
    /// The text is not TOML, so there is no table to put the path in.
    #[error("config.toml is not TOML: {message}")]
    NotToml {
        /// The parser's reason.
        message: String,
    },
    /// The folder's table is a plain value, so there is no table to put the
    /// path in without dropping it.
    #[error("config.toml holds a plain value where the folder table belongs")]
    NotATable,
}

impl NotesConfig {
    /// `text` with `notes.root` set to `root` and every other byte as it was,
    /// values Writ could not read and comments included.
    ///
    /// The one config write made while the launch holds values it could not
    /// read (ADR-048): a moved folder's files are already in their new home,
    /// so its path has to reach the file, and writing the whole config would
    /// put defaults over the values the person has to fix. A `root` already
    /// there keeps its place and its trailing comment; otherwise it joins the
    /// existing table, or a new `[notes]` table at the end.
    ///
    /// # Errors
    ///
    /// [`NotesRootEditError`] when the text is not TOML or `notes` is not a
    /// table.
    pub fn write_root_into(text: &str, root: &str) -> Result<String, NotesRootEditError> {
        let mut document = text.parse::<toml_edit::DocumentMut>().map_err(|error| {
            NotesRootEditError::NotToml {
                message: error.message().trim().to_string(),
            }
        })?;
        let notes = document
            .as_table_mut()
            .entry("notes")
            .or_insert_with(toml_edit::table);
        let table = notes
            .as_table_like_mut()
            .ok_or(NotesRootEditError::NotATable)?;
        match table.get_mut("root") {
            Some(toml_edit::Item::Value(existing)) => {
                let decor = existing.decor().clone();
                *existing = toml_edit::Value::from(root);
                *existing.decor_mut() = decor;
            }
            _ => {
                table.insert("root", toml_edit::value(root));
            }
        }
        Ok(document.to_string())
    }
}
