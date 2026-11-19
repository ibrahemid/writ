//! Where the `jotra` command is installed, what an install may replace, and
//! which `jotra` a connected program is handed (ADR-017).
//!
//! The app's install links or copies the command onto the user's PATH, and the
//! connected-programs row hands a client the path it launches. Both have to
//! name a file that is still there after Jotra quits: a quarantined macOS app
//! runs from a randomised App Translocation folder, and an AppImage runs from a
//! mount that is gone once it exits. The decisions are here; the file system
//! work is the shell's.
//!
//! The command ships under a second, legacy name that registrations and
//! scripts made before the rename run (ADR-059 decision 7). An install puts
//! `jotra` in place and keeps an existing legacy name pointed at the same
//! build.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::legacy_names;

/// The command's name on the PATH.
pub const COMMAND_NAME: &str = "jotra";

/// The suffix the command's file name carries on Windows.
const WINDOWS_SUFFIX: &str = ".exe";

/// Where the command is linked on macOS and on a Linux package install.
pub const SYSTEM_LINK: &str = "/usr/local/bin/jotra";

/// The folder, under the home folder, an AppImage copies the command into.
pub const USER_BIN: &str = ".local/bin";

/// The folder macOS runs a quarantined app from until the user moves it.
const TRANSLOCATION_FOLDER: &str = "AppTranslocation";

/// The prefix of the folder an AppImage is mounted at while it runs.
const APPIMAGE_MOUNT_PREFIX: &str = ".mount_";

/// Bytes every build of the `jotra` command carries, so a copy of it can be
/// told apart from another program that happens to be named `jotra`.
pub const CLI_SIGNATURE: &[u8] = b"com.writ.editor/writ-command";

/// Where this install of Jotra puts the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallTarget {
    /// A symbolic link at this path to the command inside the app.
    Link(PathBuf),
    /// A copy at this path: the app's own copy is gone once it quits.
    Copy(PathBuf),
    /// The installer put the command on the PATH; the app does nothing.
    ByInstaller,
}

/// What the running app knows about the place it runs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallFacts<'a> {
    /// The app runs on Windows.
    pub windows: bool,
    /// The app runs from an AppImage.
    pub in_appimage: bool,
    /// The user's home folder.
    pub home: Option<&'a Path>,
}

/// Where the command goes for an app running as `facts` describe, or `None`
/// for an AppImage with no home folder to copy into.
pub fn install_target(facts: InstallFacts<'_>) -> Option<InstallTarget> {
    if facts.windows {
        return Some(InstallTarget::ByInstaller);
    }
    if facts.in_appimage {
        return facts
            .home
            .map(|home| InstallTarget::Copy(home.join(USER_BIN).join(COMMAND_NAME)));
    }
    Some(InstallTarget::Link(PathBuf::from(SYSTEM_LINK)))
}

/// Whether `exe` runs from macOS App Translocation: a quarantined app opened
/// where it was downloaded runs from a random folder that changes with every
/// launch, so nothing may be pointed at it.
pub fn is_translocated(exe: &Path) -> bool {
    exe.components().any(
        |component| matches!(component, Component::Normal(name) if name == TRANSLOCATION_FOLDER),
    )
}

/// Whether `path` lies inside a running AppImage's mount.
pub fn is_in_appimage_mount(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(component, Component::Normal(name)
            if name.to_string_lossy().starts_with(APPIMAGE_MOUNT_PREFIX))
    })
}

/// Whether `name` is a file name the command ships under: `jotra` or the
/// legacy name, either with the Windows suffix.
pub fn is_command_file_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let stem = name.strip_suffix(WINDOWS_SUFFIX).unwrap_or(name);
    stem == COMMAND_NAME || stem == legacy_names::COMMAND_NAME
}

/// Whether a link to `target` is one an earlier Jotra install made.
///
/// Those links point at the command, under either of its names, inside an
/// app: `<name>.app/Contents/MacOS/jotra` on macOS, `usr/bin/jotra` inside an
/// AppImage mount, or a `jotra` that sits beside the app binary
/// (`beside_app`), as a Linux package installs it. A link into a bundle that
/// has since been deleted still qualifies, so a stale install can be replaced.
pub fn is_jotra_command_target(target: &Path, beside_app: bool) -> bool {
    if !target.file_name().is_some_and(is_command_file_name) {
        return false;
    }
    if beside_app || is_in_appimage_mount(target) {
        return true;
    }
    let mut ancestors = target.ancestors().skip(1);
    let in_macos_folder = ancestors
        .next()
        .is_some_and(|dir| dir.file_name().is_some_and(|name| name == "MacOS"));
    let in_contents = ancestors
        .next()
        .is_some_and(|dir| dir.file_name().is_some_and(|name| name == "Contents"));
    let in_app = ancestors.next().is_some_and(|dir| {
        dir.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    });
    in_macos_folder && in_contents && in_app
}

/// Whether `bytes` are a build of the `jotra` command ([`CLI_SIGNATURE`]).
pub fn carries_cli_signature(bytes: &[u8]) -> bool {
    bytes
        .windows(CLI_SIGNATURE.len())
        .any(|window| window == CLI_SIGNATURE)
}

/// What sits at the path the command is installed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledEntry {
    /// Nothing.
    Absent,
    /// A symbolic link to `target`, resolved against the link's folder.
    Link {
        /// Where the link points.
        target: PathBuf,
        /// Whether the app binary sits beside that target.
        beside_app: bool,
    },
    /// A file, and whether it is a build of the `jotra` command.
    File {
        /// The file carries [`CLI_SIGNATURE`].
        is_jotra_command: bool,
    },
    /// A folder or anything else the install must not touch.
    Other,
}

/// Whether an install may put the command where `entry` is. Anything that is
/// not Jotra's own command is left alone: a different tool named `jotra` is
/// never deleted.
pub fn may_replace(entry: &InstalledEntry) -> bool {
    match entry {
        InstalledEntry::Absent => true,
        InstalledEntry::Link { target, beside_app } => is_jotra_command_target(target, *beside_app),
        InstalledEntry::File { is_jotra_command } => *is_jotra_command,
        InstalledEntry::Other => false,
    }
}

/// Where an install finds the command's legacy name: beside the path it puts
/// `jotra` at, or `None` when the installer owns both names.
pub fn legacy_command_path(target: &InstallTarget) -> Option<PathBuf> {
    match target {
        InstallTarget::Link(path) | InstallTarget::Copy(path) => {
            Some(path.with_file_name(legacy_names::COMMAND_NAME))
        }
        InstallTarget::ByInstaller => None,
    }
}

/// Whether an install points the legacy name at `entry` to the current build.
///
/// Only a name already there is kept current, and only when [`may_replace`]
/// allows it: an install creates `jotra` and never adds the legacy name.
pub fn should_refresh_legacy_command(entry: &InstalledEntry) -> bool {
    !matches!(entry, InstalledEntry::Absent) && may_replace(entry)
}

/// What the running app knows when it hands a client the command to launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientCommandFacts<'a> {
    /// The running app's own executable.
    pub exe: Option<&'a Path>,
    /// The command bundled beside it, when there is one.
    pub bundled: Option<&'a Path>,
    /// Where the install puts the command.
    pub target: Option<&'a InstallTarget>,
    /// Whether the installed command is in place.
    pub installed: bool,
}

/// The `jotra` a connected program is handed, and what has to happen first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCommand {
    /// The command's path.
    pub path: PathBuf,
    /// The command is copied out of the app only when installed, and is not yet.
    pub needs_install: bool,
    /// The app runs from a translocated folder and has to be moved first.
    pub needs_move: bool,
}

/// The command handed to a client: the installed path wherever the app's own
/// copy disappears when it quits, otherwise the copy bundled with the running
/// build, and the install target when nothing is bundled (a development run).
pub fn client_command(facts: ClientCommandFacts<'_>) -> ClientCommand {
    let needs_move = facts.exe.is_some_and(is_translocated);
    if let Some(InstallTarget::Copy(path)) = facts.target {
        return ClientCommand {
            path: path.clone(),
            needs_install: !facts.installed,
            needs_move,
        };
    }
    let path = match (facts.bundled, facts.target) {
        (Some(bundled), _) => bundled.to_path_buf(),
        (None, Some(InstallTarget::Link(path))) => path.clone(),
        (None, Some(InstallTarget::ByInstaller)) => PathBuf::from("jotra.exe"),
        (None, _) => PathBuf::from(SYSTEM_LINK),
    };
    ClientCommand {
        path,
        needs_install: false,
        needs_move,
    }
}
