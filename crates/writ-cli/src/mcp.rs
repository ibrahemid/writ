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
use std::time::Duration;

use writ_core::open_context::OpenContext;
use writ_core::write_review::{
    ReviewAnswer, ReviewUnavailable, WriteProposal, WriteReview, REVIEW_WAIT,
};
use writ_mcp::tools::{AppNotesFolder, ContextSource, WritNotReached, WritOpener};

use crate::handoff::{self, Timings};
use crate::resolve_app_notes_dir;

/// What `writ mcp --help` says beneath the usage line.
pub const LONG_ABOUT: &str = "\
Serves your folder to an MCP client over stdin and stdout.
The client launches this command; it opens no port.

Turn it on in Writ under Settings, Apps, Connected programs, and
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

/// How long `open_in_writ` waits for Writ to take its link: a tool call holds
/// the client until it answers, so a Writ that is starting gets half the
/// minute a request stays fresh.
pub const OPEN_TIMINGS: Timings = Timings {
    running: Duration::from_secs(10),
    launched: Duration::from_secs(30),
    poll: Duration::from_millis(100),
};

/// Shows a file in Writ for `open_in_writ`: the link goes to the app as a
/// request in the data folder (ADR-033, ADR-052), and `launch` starts the app
/// when nothing holds the folder.
pub struct LinkHandoff<L> {
    /// The data folder the app holds.
    pub writ_dir: PathBuf,
    /// Starts the app, answering whether it did.
    pub launch: L,
}

impl<L> WritOpener for LinkHandoff<L>
where
    L: Fn() -> bool + Send + Sync,
{
    fn open_link(&self, link: &str) -> Result<(), WritNotReached> {
        handoff::deliver_link(&self.writ_dir, link, &self.launch, OPEN_TIMINGS)
            .map_err(|_| WritNotReached)
    }
}

/// How long `get_context` waits for the running Writ to say which file is
/// open.
pub const CONTEXT_WAIT: Duration = Duration::from_secs(5);

/// How often `get_context` looks for the answer while it waits.
pub const CONTEXT_POLL: Duration = Duration::from_millis(25);

/// Asks the running Writ which file is open for `get_context`, through the
/// data folder (ADR-053): a request in `open-requests/` and the answer the app
/// leaves in `context-answers/`. No socket, and nothing looked at between
/// calls.
pub struct ContextHandoff {
    /// The data folder the app holds.
    pub writ_dir: PathBuf,
}

impl ContextSource for ContextHandoff {
    fn open_context(&self, program: &str) -> Result<OpenContext, WritNotReached> {
        handoff::ask_context(&self.writ_dir, program, CONTEXT_WAIT, CONTEXT_POLL)
            .map_err(|_| WritNotReached)
    }
}

/// How often a held write looks for the person's answer.
pub const REVIEW_POLL: Duration = Duration::from_millis(100);

/// Shows a program's held write in the running Writ and waits for the
/// person's answer (ADR-053), through the data folder: the proposal in
/// `write-reviews/`, a request in `open-requests/`, and the answer the app
/// leaves in `review-answers/`. No socket, and nothing looked at between
/// writes.
pub struct ReviewHandoff {
    /// The data folder the app holds.
    pub writ_dir: PathBuf,
}

impl WriteReview for ReviewHandoff {
    fn review(&self, proposal: &WriteProposal) -> Result<ReviewAnswer, ReviewUnavailable> {
        handoff::ask_review(&self.writ_dir, proposal, REVIEW_WAIT, REVIEW_POLL)
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
