//! Startup-failure reporting policy.
//!
//! Writ can fail before it owns a window, a dialog plugin, or a log file:
//! the data directory may be unwritable, the database may be locked, a
//! migration may fail. This module owns what the resulting report says and
//! which of the candidate directories it is written to. The adapter
//! supplies the facts (the error text, the timestamp, the candidate
//! directories) and performs the I/O.

use std::path::{Path, PathBuf};

/// Title of the dialog shown when Writ cannot finish starting.
pub const FAILURE_DIALOG_TITLE: &str = "Writ could not start";

/// The startup step that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupStage {
    /// Resolving the directory Writ keeps its data in.
    DataDirectory,
    /// Opening the data directory, database, and settings.
    AppState,
}

impl StartupStage {
    /// What Writ was doing, phrased for the person reading the report.
    pub fn describe(self) -> &'static str {
        match self {
            StartupStage::DataDirectory => "resolving the data directory",
            StartupStage::AppState => "opening the data directory, database and settings",
        }
    }

    /// What the reader can do about it.
    pub fn remedy(self) -> &'static str {
        match self {
            StartupStage::DataDirectory => {
                "Writ keeps its data under your home directory. Set HOME or WRIT_DATA_DIR to a \
                 directory Writ can write to, then start Writ again."
            }
            StartupStage::AppState => {
                "Writ keeps its buffers, settings and database in that directory. Make it \
                 readable and writable, or set WRIT_DATA_DIR to another directory, then start \
                 Writ again."
            }
        }
    }
}

/// Everything known about a start that could not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupFailure {
    /// The step that failed.
    pub stage: StartupStage,
    /// The underlying error, rendered by the adapter.
    pub error: String,
    /// The path the step was working on, when there is one.
    pub path: Option<PathBuf>,
    /// Timestamp of the failure, used in the report and its file name.
    pub timestamp: String,
}

impl StartupFailure {
    /// Records a failure at `stage`.
    pub fn new(
        stage: StartupStage,
        error: impl Into<String>,
        path: Option<PathBuf>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            error: error.into(),
            path,
            timestamp: timestamp.into(),
        }
    }

    /// File name for the report: `writ-crash-<timestamp>.txt`.
    ///
    /// Prefixed because the fallback location is the shared system
    /// temporary directory, where an unprefixed name says nothing about
    /// which program wrote it.
    pub fn report_file_name(&self) -> String {
        format!("writ-crash-{}.txt", self.timestamp)
    }
}

/// Chooses where the failure report is written.
///
/// `logs_dir` is `Some` when the caller has one to try. The system temporary
/// directory is the fallback because the logs directory sits inside the data
/// directory, and that directory being unwritable is the likeliest reason
/// startup failed in the first place.
pub fn choose_report_path(logs_dir: Option<&Path>, temp_dir: &Path, file_name: &str) -> PathBuf {
    match logs_dir {
        Some(dir) => dir.join(file_name),
        None => temp_dir.join(file_name),
    }
}

/// Renders the report written to disk.
///
/// `report_path` is where that file lands; `None` records that no writable
/// location was found.
pub fn format_failure_report(failure: &StartupFailure, report_path: Option<&Path>) -> String {
    let mut out = String::from("Writ could not start.\n\n");
    out.push_str(&detail_block(failure));
    out.push_str(&format!("Time: {}\n\n", failure.timestamp));
    out.push_str(failure.stage.remedy());
    out.push_str("\n\n");
    out.push_str(&report_line(report_path));
    out.push_str(
        "Writ also prints this report to standard error, which a desktop launch discards.\n",
    );
    out
}

/// Renders the dialog body.
///
/// Shorter than the file report: a system alert has no scrollback, so it
/// carries the failure, the remedy, and the path to the full report.
pub fn format_failure_dialog(failure: &StartupFailure, report_path: Option<&Path>) -> String {
    let mut out = detail_block(failure);
    out.push('\n');
    out.push_str(failure.stage.remedy());
    out.push_str("\n\n");
    out.push_str(&report_line(report_path));
    out
}

fn detail_block(failure: &StartupFailure) -> String {
    let mut out = format!("Step: {}\n", failure.stage.describe());
    if let Some(path) = &failure.path {
        out.push_str(&format!("Path: {}\n", path.display()));
    }
    out.push_str(&format!("Error: {}\n", failure.error));
    out
}

fn report_line(report_path: Option<&Path>) -> String {
    match report_path {
        Some(path) => format!("Report file: {}\n", path.display()),
        None => String::from("Report file: none, no writable location was found for it.\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure() -> StartupFailure {
        StartupFailure::new(
            StartupStage::AppState,
            "Permission denied (os error 13)",
            Some(PathBuf::from("/home/user/.writ")),
            "20260730-101500",
        )
    }

    #[test]
    fn report_file_name_carries_the_timestamp_and_a_writ_prefix() {
        assert_eq!(
            failure().report_file_name(),
            "writ-crash-20260730-101500.txt"
        );
    }

    #[test]
    fn choose_report_path_prefers_the_logs_dir() {
        let path = choose_report_path(
            Some(Path::new("/home/user/.writ/logs")),
            Path::new("/tmp"),
            "writ-crash-1.txt",
        );
        assert_eq!(
            path,
            PathBuf::from("/home/user/.writ/logs/writ-crash-1.txt")
        );
    }

    #[test]
    fn choose_report_path_falls_back_to_the_temp_dir() {
        let path = choose_report_path(None, Path::new("/tmp"), "writ-crash-1.txt");
        assert_eq!(path, PathBuf::from("/tmp/writ-crash-1.txt"));
    }

    #[test]
    fn report_names_the_step_the_path_and_the_error() {
        let report = format_failure_report(&failure(), Some(Path::new("/tmp/writ-crash-1.txt")));
        assert!(report.contains("Step: opening the data directory, database and settings"));
        assert!(report.contains("Path: /home/user/.writ"));
        assert!(report.contains("Error: Permission denied (os error 13)"));
        assert!(report.contains("Time: 20260730-101500"));
    }

    #[test]
    fn report_states_where_it_was_written() {
        let report = format_failure_report(&failure(), Some(Path::new("/tmp/writ-crash-1.txt")));
        assert!(report.contains("Report file: /tmp/writ-crash-1.txt"));
    }

    #[test]
    fn report_states_when_it_could_not_be_written() {
        let report = format_failure_report(&failure(), None);
        assert!(report.contains("Report file: none, no writable location was found for it."));
    }

    #[test]
    fn report_omits_the_path_line_when_there_is_no_path() {
        let failure = StartupFailure::new(
            StartupStage::DataDirectory,
            "could not find home directory",
            None,
            "20260730-101500",
        );
        let report = format_failure_report(&failure, None);
        assert!(!report.contains("Path:"));
        assert!(report.contains("Step: resolving the data directory"));
        assert!(report.contains("Set HOME or WRIT_DATA_DIR"));
    }

    #[test]
    fn dialog_body_carries_the_failure_remedy_and_report_path() {
        let body = format_failure_dialog(&failure(), Some(Path::new("/tmp/writ-crash-1.txt")));
        assert!(body.contains("Error: Permission denied (os error 13)"));
        assert!(body.contains("Make it readable and writable"));
        assert!(body.contains("Report file: /tmp/writ-crash-1.txt"));
    }

    #[test]
    fn dialog_body_drops_the_standard_error_note_the_file_carries() {
        let path = Path::new("/tmp/writ-crash-1.txt");
        let body = format_failure_dialog(&failure(), Some(path));
        assert!(!body.contains("standard error"));
        assert!(format_failure_report(&failure(), Some(path)).contains("standard error"));
    }
}
