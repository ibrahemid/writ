//! Startup-failure reporting policy.
//!
//! Writ can fail before it owns a window, a dialog plugin, or a log file:
//! the data directory may be unwritable, the database may be locked, a
//! migration may fail. This module owns what the resulting report says and
//! which of the candidate directories it is written to. The adapter
//! supplies the facts (the error text, the timestamp, the candidate
//! directories) and performs the I/O.

use std::path::{Path, PathBuf};

use crate::notes::DEFAULT_NOTES_FOLDER;

/// Title of the dialog shown when Writ cannot finish starting.
pub const FAILURE_DIALOG_TITLE: &str = "Writ could not start";

/// The startup step that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupStage {
    /// Resolving the directory Writ keeps its data in.
    DataDirectory,
    /// Checking that the resolved data directory is somewhere a database can
    /// live.
    DataDirectoryLocation,
    /// Opening the data directory, database, and settings.
    AppState,
}

impl StartupStage {
    /// What Writ was doing, phrased for the person reading the report.
    pub fn describe(self) -> &'static str {
        match self {
            StartupStage::DataDirectory => "resolving the data directory",
            StartupStage::DataDirectoryLocation => "checking where Writ keeps its data",
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
            StartupStage::DataDirectoryLocation => {
                "Set WRIT_DATA_DIR to a folder outside the synced folder, then start Writ again."
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

/// Which platform's table of synced folders applies.
///
/// Passed as data rather than read from `cfg!` inline so all three tables are
/// exercised from one host, the way [`crate::default_app`] and the reveal
/// command already do it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux and the other Unixes.
    Linux,
}

/// A sync provider whose tree must not hold Writ's database.
///
/// SQLite's write-ahead log needs the shared memory file, the log and the
/// database to stay consistent with each other; a provider that uploads them
/// separately, materialises them on demand, or resolves a conflict by keeping
/// two copies breaks that. Every vendor documents the same warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncProvider {
    /// Apple iCloud Drive.
    ICloud,
    /// Dropbox.
    Dropbox,
    /// Google Drive.
    GoogleDrive,
    /// Microsoft OneDrive.
    OneDrive,
    /// Syncthing, found by its `.stfolder` marker rather than by a name.
    Syncthing,
}

impl SyncProvider {
    /// The provider's name, as the user knows it.
    pub fn label(self) -> &'static str {
        match self {
            SyncProvider::ICloud => "iCloud Drive",
            SyncProvider::Dropbox => "Dropbox",
            SyncProvider::GoogleDrive => "Google Drive",
            SyncProvider::OneDrive => "OneDrive",
            SyncProvider::Syncthing => "Syncthing",
        }
    }
}

/// Why the data directory cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDirVerdict {
    /// The directory is usable.
    Ok,
    /// The directory sits under a sync provider's tree.
    InsideSyncProvider {
        /// The provider whose tree it is.
        provider: SyncProvider,
        /// The provider's folder, as far up as it was recognised.
        root: PathBuf,
    },
    /// The data directory and the notes folder hold each other.
    InsideNotesFolder {
        /// The notes folder the two were compared against.
        notes_root: PathBuf,
    },
}

/// A data-directory verdict that stops the launch.
///
/// Carries the verdict so the caller renders one message; the `Display` text
/// is [`data_dir_refusal_message`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", data_dir_refusal_message(.0))]
pub struct DataDirRefused(pub DataDirVerdict);

/// The folder names each platform's providers use, relative to the home
/// directory, in path components.
///
/// macOS `Library/CloudStorage` is Apple's File Provider area rather than one
/// vendor's folder, so the provider it maps to here is a default that
/// [`provider_in_cloud_storage`] refines from the container name below it.
fn sync_prefixes(platform: Platform) -> &'static [(&'static [&'static str], SyncProvider)] {
    match platform {
        Platform::Macos => &[
            (&["Library", "Mobile Documents"], SyncProvider::ICloud),
            (&["Library", "CloudStorage"], SyncProvider::ICloud),
            (&["Dropbox"], SyncProvider::Dropbox),
            (&["Google Drive"], SyncProvider::GoogleDrive),
        ],
        Platform::Windows => &[
            (&["OneDrive"], SyncProvider::OneDrive),
            (&["Dropbox"], SyncProvider::Dropbox),
            (&["Google Drive"], SyncProvider::GoogleDrive),
        ],
        Platform::Linux => &[
            (&["Dropbox"], SyncProvider::Dropbox),
            (&["Google Drive"], SyncProvider::GoogleDrive),
        ],
    }
}

/// Reads the vendor out of a `Library/CloudStorage` container name.
///
/// The names are minted by the vendor's File Provider extension and carry an
/// account suffix (`GoogleDrive-me@example.com`, `OneDrive-Personal`), so the
/// match is on the leading name only. An unrecognised container is still a
/// synced folder; it keeps the default the table gave it.
fn provider_in_cloud_storage(container: &str, default: SyncProvider) -> SyncProvider {
    for (name, provider) in [
        ("Dropbox", SyncProvider::Dropbox),
        ("GoogleDrive", SyncProvider::GoogleDrive),
        ("Google Drive", SyncProvider::GoogleDrive),
        ("OneDrive", SyncProvider::OneDrive),
    ] {
        if container.starts_with(name) {
            return provider;
        }
    }
    default
}

/// Classifies a resolved data directory.
///
/// `markers` is the set of directories found to hold a `.stfolder` marker
/// (Syncthing), supplied by the adapter so this stays free of I/O. They are
/// checked on every platform: Syncthing runs on all three and names its
/// folders whatever the user named them, so the marker is the only signal.
///
/// `notes_root` is passed only once it has been resolved, which is later in
/// the launch than the first call. `<data_dir>/`[`DEFAULT_NOTES_FOLDER`] is
/// exempt from the containment check, because that is what
/// [`crate::notes::resolve_notes_root_from`] resolves to whenever a data-folder
/// override is in force; refusing it would stop every instance running against
/// its own data folder from starting.
pub fn classify_data_dir(
    platform: Platform,
    data_dir: &Path,
    home: Option<&Path>,
    notes_root: Option<&Path>,
    stfolder_markers: &[PathBuf],
) -> DataDirVerdict {
    for marker in stfolder_markers {
        if data_dir.starts_with(marker) {
            return DataDirVerdict::InsideSyncProvider {
                provider: SyncProvider::Syncthing,
                root: marker.clone(),
            };
        }
    }

    if let Some(home) = home {
        for (components, default) in sync_prefixes(platform) {
            let mut root = home.to_path_buf();
            for component in *components {
                root.push(component);
            }
            let Ok(rest) = data_dir.strip_prefix(&root) else {
                continue;
            };
            let mut provider = *default;
            if components.last() == Some(&"CloudStorage") {
                if let Some(container) = rest.components().next() {
                    let container = container.as_os_str().to_string_lossy();
                    provider = provider_in_cloud_storage(&container, provider);
                    root.push(container.as_ref());
                }
            }
            return DataDirVerdict::InsideSyncProvider { provider, root };
        }
    }

    if let Some(notes_root) = notes_root {
        let data_dir_inside = data_dir.starts_with(notes_root);
        let notes_inside =
            notes_root.starts_with(data_dir) && notes_root != data_dir.join(DEFAULT_NOTES_FOLDER);
        if data_dir_inside || notes_inside {
            return DataDirVerdict::InsideNotesFolder {
                notes_root: notes_root.to_path_buf(),
            };
        }
    }

    DataDirVerdict::Ok
}

/// The plain-language refusal shown to the user.
///
/// [`DataDirVerdict::Ok`] has no message: the caller renders one only for a
/// verdict that stops the launch.
pub fn data_dir_refusal_message(verdict: &DataDirVerdict) -> String {
    match verdict {
        DataDirVerdict::Ok => String::new(),
        DataDirVerdict::InsideSyncProvider { provider, root } => format!(
            "Writ's data folder is inside {}, which {} syncs. A synced folder can damage the \
             database and lose notes, so Writ will not start there. Set WRIT_DATA_DIR to a folder \
             outside {}, then start Writ again.",
            root.display(),
            provider.label(),
            provider.label()
        ),
        DataDirVerdict::InsideNotesFolder { notes_root } => format!(
            "Writ's data folder and your notes folder, {}, overlap. The database and your notes \
             cannot share a folder. Set WRIT_DATA_DIR to a folder outside your notes folder, or \
             pick another notes folder in Settings, then start Writ again.",
            notes_root.display()
        ),
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
    fn the_location_report_carries_the_verdict_and_the_remedy() {
        let verdict = DataDirVerdict::InsideSyncProvider {
            provider: SyncProvider::Dropbox,
            root: PathBuf::from("/home/user/Dropbox"),
        };
        let failure = StartupFailure::new(
            StartupStage::DataDirectoryLocation,
            data_dir_refusal_message(&verdict),
            Some(PathBuf::from("/home/user/Dropbox/.writ")),
            "20260901-101500",
        );
        let report = format_failure_report(&failure, Some(Path::new("/tmp/writ-crash-1.txt")));
        assert!(report.contains("Step: checking where Writ keeps its data"));
        assert!(report.contains("Path: /home/user/Dropbox/.writ"));
        assert!(report.contains("which Dropbox syncs"));
        assert!(report.contains("Set WRIT_DATA_DIR to a folder outside the synced folder"));
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
