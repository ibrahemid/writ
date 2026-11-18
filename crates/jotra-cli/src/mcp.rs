//! `jotra mcp`: the MCP server, over this process's stdin and stdout.
//!
//! A client launches this command and speaks the protocol down the pipe. The
//! app hosts nothing: a window has no client stdio to attach to, so "shipped
//! with the app" means the app ships this binary and shows the command to paste
//! into a client's configuration (ADR-031 rule 2.4).
//!
//! The server reads `jotra.db` read-only and creates nothing. Two processes do
//! not write one SQLite file, so a folder Jotra has never opened has no index
//! and the index-derived tools say so until the app builds one. The server
//! serves the app's notes folder and follows it when the settings move it,
//! unless `JOTRA_NOTES_DIR` names another.

use std::path::PathBuf;
use std::time::Duration;

use jotra_core::open_context::OpenContext;
use jotra_core::write_review::{
    ReviewAnswer, ReviewUnavailable, WriteProposal, WriteReview, REVIEW_WAIT,
};
use jotra_mcp::tools::{AppNotesFolder, ContextSource, JotraNotReached, JotraOpener};

use crate::handoff::{self, Timings};
use crate::resolve_app_notes_dir;

/// What `jotra mcp --help` says beneath the usage line.
pub const LONG_ABOUT: &str = "\
Serves your folder to an MCP client over stdin and stdout.
The client launches this command; it opens no port.

Turn it on in Jotra under Settings, Apps, Connected programs, and
approve the program there. Until then no tool call is answered.";

/// The environment `jotra mcp` reads, printed after its help.
pub const ENV_HELP: &str = "\
Environment:
  JOTRA_NOTES_DIR  The folder to read, overriding the setting.
  JOTRA_DATA_DIR   The folder holding jotra.db and config.toml.";

/// The app's notes folder, resolved from its settings at every write the
/// server keeps a version of ([`resolve_app_notes_dir`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredAppFolder {
    /// The data folder holding `config.toml`.
    pub jotra_dir: PathBuf,
    /// The data folder's notes default
    /// ([`jotra_core::data_folder::DataFolder::notes_default`]).
    pub notes_default: Option<PathBuf>,
    /// The user's home folder.
    pub home: Option<PathBuf>,
}

impl AppNotesFolder for ConfiguredAppFolder {
    fn app_notes_root(&self) -> Option<PathBuf> {
        resolve_app_notes_dir(
            &self.jotra_dir,
            self.notes_default.as_deref(),
            self.home.as_deref(),
        )
    }
}

/// How long `open_in_jotra` waits for Jotra to take its link: a tool call holds
/// the client until it answers, so a Jotra that is starting gets half the
/// minute a request stays fresh.
pub const OPEN_TIMINGS: Timings = Timings {
    running: Duration::from_secs(10),
    launched: Duration::from_secs(30),
    poll: Duration::from_millis(100),
};

/// Shows a file in Jotra for `open_in_jotra`: the link goes to the app as a
/// request in the data folder (ADR-033, ADR-052), and `launch` starts the app
/// when nothing holds the folder.
pub struct LinkHandoff<L> {
    /// The data folder the app holds.
    pub jotra_dir: PathBuf,
    /// Starts the app, answering whether it did.
    pub launch: L,
}

impl<L> JotraOpener for LinkHandoff<L>
where
    L: Fn() -> bool + Send + Sync,
{
    fn open_link(&self, link: &str) -> Result<(), JotraNotReached> {
        handoff::deliver_link(&self.jotra_dir, link, &self.launch, OPEN_TIMINGS)
            .map_err(|_| JotraNotReached)
    }
}

/// How long `get_context` waits for the running Jotra to say which file is
/// open.
pub const CONTEXT_WAIT: Duration = Duration::from_secs(5);

/// How often `get_context` looks for the answer while it waits.
pub const CONTEXT_POLL: Duration = Duration::from_millis(25);

/// Asks the running Jotra which file is open for `get_context`, through the
/// data folder (ADR-053): a request in `open-requests/` and the answer the app
/// leaves in `context-answers/`. No socket, and nothing looked at between
/// calls.
pub struct ContextHandoff {
    /// The data folder the app holds.
    pub jotra_dir: PathBuf,
}

impl ContextSource for ContextHandoff {
    fn open_context(&self, program: &str) -> Result<OpenContext, JotraNotReached> {
        handoff::ask_context(&self.jotra_dir, program, CONTEXT_WAIT, CONTEXT_POLL)
            .map_err(|_| JotraNotReached)
    }
}

/// How often a held write looks for the person's answer.
pub const REVIEW_POLL: Duration = Duration::from_millis(100);

/// Shows a program's held write in the running Jotra and waits for the
/// person's answer (ADR-053), through the data folder: the proposal in
/// `write-reviews/`, a request in `open-requests/`, and the answer the app
/// leaves in `review-answers/`. No socket, and nothing looked at between
/// writes.
pub struct ReviewHandoff {
    /// The data folder the app holds.
    pub jotra_dir: PathBuf,
}

impl WriteReview for ReviewHandoff {
    fn review(&self, proposal: &WriteProposal) -> Result<ReviewAnswer, ReviewUnavailable> {
        handoff::ask_review(&self.jotra_dir, proposal, REVIEW_WAIT, REVIEW_POLL)
    }
}

/// Whether `JOTRA_NOTES_DIR`, as the environment holds it, pins the folder the
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
            jotra_dir: dir.path().to_path_buf(),
            notes_default: None,
            home: Some(dir.path().to_path_buf()),
        };
        assert_eq!(folder.app_notes_root(), Some(dir.path().join("Jotra")));

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
