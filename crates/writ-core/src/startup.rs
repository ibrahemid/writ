//! Startup-failure reporting policy.
//!
//! Writ can fail before it owns a window, a dialog plugin, or a log file:
//! the data directory may be unwritable, the database may be locked, a
//! migration may fail. This module owns what the resulting report says and
//! which of the candidate directories it is written to. The adapter
//! supplies the facts (the error text, the timestamp, the candidate
//! directories) and performs the I/O.

use std::ffi::OsStr;
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
    /// The directory sits under a `Library/CloudStorage` container whose
    /// vendor is not one of [`SyncProvider`]'s.
    ///
    /// Apple's File Provider area holds a container per vendor, so a Box or
    /// pCloud folder lands there beside Dropbox's. The container names the
    /// service, and that name is what the refusal has to say: telling a Box
    /// user to move out of iCloud Drive sends them to the wrong place.
    InsideSyncContainer {
        /// The service, as the container folder names it.
        name: String,
        /// The container folder.
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

/// Whether `platform`'s default filesystem treats two spellings that differ
/// only in case as one name.
///
/// APFS and NTFS are case-preserving but case-insensitive by default, so
/// `~/dropbox` and `~/Dropbox` are the same folder there; ext4 and the rest of
/// Linux keep them apart. This is the same rule
/// `security::paths_equal_for_authorization` follows in the adapter, decided
/// from the platform rather than from `cfg!` so every table stays testable
/// from one host.
fn folds_case(platform: Platform) -> bool {
    match platform {
        Platform::Macos | Platform::Windows => true,
        Platform::Linux => false,
    }
}

/// Compares one path component the way `folds_case` says the filesystem does.
fn names_match(a: &OsStr, b: &OsStr, fold: bool) -> bool {
    if fold {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Splits `path` at `prefix`, comparing component by component.
///
/// Returns the prefix as `path` spells it and the components below it, or
/// `None` when `path` is not inside `prefix`. The matched half comes back in
/// the caller's own spelling so a refusal names the folder the user typed
/// rather than the table's capitalisation.
fn split_prefix(path: &Path, prefix: &Path, fold: bool) -> Option<(PathBuf, PathBuf)> {
    let mut rest = path.components();
    let mut matched = PathBuf::new();
    for want in prefix.components() {
        let have = rest.next()?;
        if !names_match(have.as_os_str(), want.as_os_str(), fold) {
            return None;
        }
        matched.push(have);
    }
    Some((matched, rest.as_path().to_path_buf()))
}

/// Whether `path` is `prefix` or sits below it, folding case per platform.
fn is_within(path: &Path, prefix: &Path, fold: bool) -> bool {
    split_prefix(path, prefix, fold).is_some()
}

/// Whether two paths name the same folder, folding case per platform.
fn same_path(a: &Path, b: &Path, fold: bool) -> bool {
    a.components().count() == b.components().count() && is_within(a, b, fold)
}

/// The folder names each platform's providers use, relative to the home
/// directory, in path components.
///
/// macOS `Library/CloudStorage` is Apple's File Provider area rather than one
/// vendor's folder, so it carries no provider of its own: the container below
/// it names the service, and [`provider_in_cloud_storage`] reads it.
fn sync_prefixes(platform: Platform) -> &'static [(&'static [&'static str], Option<SyncProvider>)] {
    match platform {
        Platform::Macos => &[
            (&["Library", "Mobile Documents"], Some(SyncProvider::ICloud)),
            (&["Library", "CloudStorage"], None),
            (&["Dropbox"], Some(SyncProvider::Dropbox)),
            (&["Google Drive"], Some(SyncProvider::GoogleDrive)),
        ],
        Platform::Windows => &[
            (&["OneDrive"], Some(SyncProvider::OneDrive)),
            (&["Dropbox"], Some(SyncProvider::Dropbox)),
            (&["Google Drive"], Some(SyncProvider::GoogleDrive)),
        ],
        Platform::Linux => &[
            (&["Dropbox"], Some(SyncProvider::Dropbox)),
            (&["Google Drive"], Some(SyncProvider::GoogleDrive)),
        ],
    }
}

/// Reads the vendor out of a `Library/CloudStorage` container name.
///
/// The names are minted by the vendor's File Provider extension and carry an
/// account suffix (`GoogleDrive-me@example.com`, `OneDrive-Personal`), so the
/// match is on the leading name only. `None` for a container no variant
/// covers, which [`container_display_name`] then names from the folder.
fn provider_in_cloud_storage(container: &str, fold: bool) -> Option<SyncProvider> {
    let container = if fold {
        container.to_lowercase()
    } else {
        container.to_string()
    };
    for (name, provider) in [
        ("Dropbox", SyncProvider::Dropbox),
        ("GoogleDrive", SyncProvider::GoogleDrive),
        ("Google Drive", SyncProvider::GoogleDrive),
        ("OneDrive", SyncProvider::OneDrive),
    ] {
        let name = if fold {
            name.to_lowercase()
        } else {
            name.to_string()
        };
        if container.starts_with(&name) {
            return Some(provider);
        }
    }
    None
}

/// The service name to show for a `Library/CloudStorage` container Writ has no
/// variant for.
///
/// The vendor's File Provider extension mints the folder as `Vendor-account`
/// (`Box-me@example.com`, `pCloud-Personal`), so the name is what stands
/// before the first dash. A container with no dash is already the bare name,
/// and one that starts with a dash is shown whole rather than emptied.
fn container_display_name(container: &str) -> String {
    match container.split_once('-') {
        Some((name, _)) if !name.trim().is_empty() => name.trim().to_string(),
        _ => container.to_string(),
    }
}

/// Classifies a resolved data directory.
///
/// `markers` is the set of directories found to hold a `.stfolder` marker
/// (Syncthing), supplied by the adapter so this stays free of I/O. They are
/// checked on every platform: Syncthing runs on all three and names its
/// folders whatever the user named them, so the marker is the only signal.
///
/// Path components are compared the way `platform`'s filesystem does: folding
/// case on macOS and Windows, byte-exact on Linux. Beyond case, paths are
/// compared as spelled, so a caller that means folders rather than
/// spellings hands over resolved ones: a data folder symlinked into a synced
/// folder, or a `WRIT_DATA_DIR` written through a symlink, is only visible in
/// its canonical form. The adapter's `data_dir_verdict` asks about both.
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
    let fold = folds_case(platform);

    for marker in stfolder_markers {
        if is_within(data_dir, marker, fold) {
            return DataDirVerdict::InsideSyncProvider {
                provider: SyncProvider::Syncthing,
                root: marker.clone(),
            };
        }
    }

    if let Some(home) = home {
        for (components, default) in sync_prefixes(platform) {
            let mut prefix = home.to_path_buf();
            for component in *components {
                prefix.push(component);
            }
            let Some((mut root, rest)) = split_prefix(data_dir, &prefix, fold) else {
                continue;
            };
            let mut provider = *default;
            if components.last() == Some(&"CloudStorage") {
                // The container is what says which service owns the folder, so
                // a data directory sitting at the bare `CloudStorage` root has
                // nothing to read and is named after the folder itself.
                let container = rest
                    .components()
                    .next()
                    .map(|component| component.as_os_str().to_string_lossy().into_owned())
                    .unwrap_or_else(|| "CloudStorage".to_string());
                provider = provider_in_cloud_storage(&container, fold);
                if rest.components().next().is_some() {
                    root.push(&container);
                }
                if provider.is_none() {
                    return DataDirVerdict::InsideSyncContainer {
                        name: container_display_name(&container),
                        root,
                    };
                }
            }
            let Some(provider) = provider else {
                continue;
            };
            return DataDirVerdict::InsideSyncProvider { provider, root };
        }
    }

    if let Some(notes_root) = notes_root {
        let data_dir_inside = is_within(data_dir, notes_root, fold);
        let notes_inside = is_within(notes_root, data_dir, fold)
            && !same_path(notes_root, &data_dir.join(DEFAULT_NOTES_FOLDER), fold);
        if data_dir_inside || notes_inside {
            return DataDirVerdict::InsideNotesFolder {
                notes_root: notes_root.to_path_buf(),
            };
        }
    }

    DataDirVerdict::Ok
}

/// The refusal for a data folder inside a synced tree, whether the service came
/// from [`SyncProvider`] or from a container's own name.
fn sync_refusal_message(root: &Path, service: &str) -> String {
    format!(
        "Writ's data folder is inside {}, which {} syncs. A synced folder can damage the database \
         and lose notes, so Writ will not start there. Set WRIT_DATA_DIR to a folder outside {}, \
         then start Writ again.",
        root.display(),
        service,
        service
    )
}

/// The plain-language refusal shown to the user.
///
/// [`DataDirVerdict::Ok`] has no message: the caller renders one only for a
/// verdict that stops the launch.
pub fn data_dir_refusal_message(verdict: &DataDirVerdict) -> String {
    match verdict {
        DataDirVerdict::Ok => String::new(),
        DataDirVerdict::InsideSyncProvider { provider, root } => {
            sync_refusal_message(root, provider.label())
        }
        DataDirVerdict::InsideSyncContainer { name, root } => sync_refusal_message(root, name),
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
