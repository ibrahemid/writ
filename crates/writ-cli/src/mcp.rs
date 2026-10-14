//! `writ mcp`: the MCP server, over this process's stdin and stdout.
//!
//! A client launches this command and speaks the protocol down the pipe. The
//! app hosts nothing: a window has no client stdio to attach to, so "shipped
//! with the app" means the app ships this binary and shows the command to paste
//! into a client's configuration (ADR-031 rule 2.4).
//!
//! The server reads `writ.db` read-only and creates nothing. Two processes do
//! not write one SQLite file, so a folder Writ has never opened has no index
//! and the index-derived tools say so until the app builds one. The server
//! serves the app's notes folder and follows it when the settings move it,
//! unless `WRIT_NOTES_DIR` names another.

use std::path::PathBuf;

use writ_mcp::tools::AppNotesFolder;

use crate::resolve_app_notes_dir;

/// What `writ mcp --help` says beneath the usage line.
pub const LONG_ABOUT: &str = "\
Serves your folder to an MCP client over stdin and stdout.
The client launches this command; it opens no port.

Turn it on in Writ's settings, under Connected programs, and
approve the program there. Until then no tool call is answered.";

/// The environment `writ mcp` reads, printed after its help.
pub const ENV_HELP: &str = "\
Environment:
  WRIT_NOTES_DIR  The folder to read, overriding the setting.
  WRIT_DATA_DIR   The folder holding writ.db and config.toml.";

/// The app's notes folder, resolved from its settings at every write the
/// server keeps a version of ([`resolve_app_notes_dir`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredAppFolder {
    /// The data folder holding `config.toml`.
    pub writ_dir: PathBuf,
    /// The data folder, when `WRIT_DATA_DIR` names it.
    pub data_dir: Option<PathBuf>,
    /// The user's home folder.
    pub home: Option<PathBuf>,
}

impl AppNotesFolder for ConfiguredAppFolder {
    fn app_notes_root(&self) -> Option<PathBuf> {
        resolve_app_notes_dir(
            &self.writ_dir,
            self.data_dir.as_deref(),
            self.home.as_deref(),
        )
    }
}

/// Whether `WRIT_NOTES_DIR`, as the environment holds it, pins the folder the
/// server serves. Pinned, the server answers from that folder whatever the
/// settings name; otherwise it follows the app's folder as the settings move
/// it. A blank value names no folder, as the folder resolution reads it.
pub fn notes_dir_is_pinned(env_value: Option<&str>) -> bool {
    env_value.is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_folder_is_read_from_the_settings_at_every_ask() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let config = dir.path().join("config.toml");
        let folder = ConfiguredAppFolder {
            writ_dir: dir.path().to_path_buf(),
            data_dir: None,
            home: Some(dir.path().to_path_buf()),
        };
        assert_eq!(folder.app_notes_root(), Some(dir.path().join("Writ")));

        let moved = dir.path().join("Moved");
        std::fs::write(&config, format!("[notes]\nroot = '{}'\n", moved.display()))
            .expect("write the settings");

        assert_eq!(folder.app_notes_root(), Some(moved));
    }

    #[test]
    fn only_a_folder_named_in_the_environment_pins_the_server() {
        assert!(notes_dir_is_pinned(Some("/tmp/Notes")));
        assert!(!notes_dir_is_pinned(Some("  ")));
        assert!(!notes_dir_is_pinned(Some("")));
        assert!(!notes_dir_is_pinned(None));
    }
}
