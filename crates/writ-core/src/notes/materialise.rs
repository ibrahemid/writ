//! Whether opening a note may read it, and how long its download is waited on.
//!
//! A sync provider can leave a note as a placeholder with no local data behind
//! it. Reading one asks the provider daemon to fetch it, which blocks the
//! reading thread for as long as the network takes. The open path therefore
//! asks this module first, and a placeholder is downloaded on a thread of its
//! own instead of stalling the open.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a download is waited on before Writ stops waiting and says so.
///
/// The wait is bounded by the provider and the connection, not by anything
/// Writ does, so there is no right answer read off the file. Three minutes
/// covers a large note on a slow link, and is short enough that a provider
/// that is paused, signed out or offline is reported rather than waited on for
/// the rest of the session.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);

/// What an open of a note may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenDecision {
    /// The bytes are on this machine. Read the file.
    Read,
    /// The file is a placeholder. Nothing is read on the caller's thread; the
    /// bytes are asked for separately.
    Download {
        /// The file to download, as the caller named it.
        path: PathBuf,
    },
}

/// Decides an open from the path and what a stat said about its data.
///
/// `dataless` is the answer of a metadata-only probe. Nothing here opens the
/// file: deciding whether reading a file is safe by reading it is the stall
/// this exists to avoid.
pub fn decide_open(path: &Path, dataless: bool) -> OpenDecision {
    if dataless {
        OpenDecision::Download {
            path: path.to_path_buf(),
        }
    } else {
        OpenDecision::Read
    }
}

/// How a download ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// The bytes are on this machine now.
    Done,
    /// The person stopped waiting.
    Cancelled,
    /// The provider could not produce the file.
    Failed(String),
    /// [`DOWNLOAD_TIMEOUT`] passed with the file still not here.
    TimedOut,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_with_its_bytes_here_is_read() {
        assert_eq!(
            decide_open(Path::new("/notes/a.md"), false),
            OpenDecision::Read
        );
    }

    #[test]
    fn a_placeholder_is_downloaded_rather_than_read() {
        assert_eq!(
            decide_open(Path::new("/notes/a.md"), true),
            OpenDecision::Download {
                path: PathBuf::from("/notes/a.md"),
            }
        );
    }

    #[test]
    fn the_download_decision_carries_the_path_it_was_given() {
        let OpenDecision::Download { path } = decide_open(Path::new("/notes/sub/b.md"), true)
        else {
            panic!("a placeholder should be downloaded");
        };
        assert_eq!(path, PathBuf::from("/notes/sub/b.md"));
    }

    #[test]
    fn the_timeout_is_three_minutes() {
        assert_eq!(DOWNLOAD_TIMEOUT, Duration::from_secs(3 * 60));
    }
}
