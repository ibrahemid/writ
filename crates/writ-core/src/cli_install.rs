//! Where the `writ` command is installed, what an install may replace, and
//! which `writ` a connected program is handed (ADR-017).
//!
//! The app's install links or copies the command onto the user's PATH, and the
//! connected-programs row hands a client the path it launches. Both have to
//! name a file that is still there after Writ quits: a quarantined macOS app
//! runs from a randomised App Translocation folder, and an AppImage runs from a
//! mount that is gone once it exits. The decisions are here; the file system
//! work is the shell's.

use std::path::{Component, Path, PathBuf};

/// Where the command is linked on macOS and on a Linux package install.
pub const SYSTEM_LINK: &str = "/usr/local/bin/writ";

/// The folder, under the home folder, an AppImage copies the command into.
pub const USER_BIN: &str = ".local/bin";

/// The folder macOS runs a quarantined app from until the user moves it.
const TRANSLOCATION_FOLDER: &str = "AppTranslocation";

/// The prefix of the folder an AppImage is mounted at while it runs.
const APPIMAGE_MOUNT_PREFIX: &str = ".mount_";

/// Bytes every build of the `writ` command carries, so a copy of it can be
/// told apart from another program that happens to be named `writ`.
pub const CLI_SIGNATURE: &[u8] = b"com.writ.editor/writ-command";

/// Where this install of Writ puts the command.
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
            .map(|home| InstallTarget::Copy(home.join(USER_BIN).join("writ")));
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

/// Whether a link to `target` is one an earlier Writ install made.
///
/// Those links point at the command inside an app: `<name>.app/Contents/MacOS/writ`
/// on macOS, `usr/bin/writ` inside an AppImage mount, or a `writ` that sits
/// beside the app binary (`beside_app`), as a Linux package installs it. A
/// link into a bundle that has since been deleted still qualifies, so a stale
/// install can be replaced.
pub fn is_writ_command_target(target: &Path, beside_app: bool) -> bool {
    let named_writ = target
        .file_name()
        .is_some_and(|name| name == "writ" || name == "writ.exe");
    if !named_writ {
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

/// Whether `bytes` are a build of the `writ` command ([`CLI_SIGNATURE`]).
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
    /// A file, and whether it is a build of the `writ` command.
    File {
        /// The file carries [`CLI_SIGNATURE`].
        is_writ_command: bool,
    },
    /// A folder or anything else the install must not touch.
    Other,
}

/// Whether an install may put the command where `entry` is. Anything that is
/// not Writ's own command is left alone: a different tool named `writ` is
/// never deleted.
pub fn may_replace(entry: &InstalledEntry) -> bool {
    match entry {
        InstalledEntry::Absent => true,
        InstalledEntry::Link { target, beside_app } => is_writ_command_target(target, *beside_app),
        InstalledEntry::File { is_writ_command } => *is_writ_command,
        InstalledEntry::Other => false,
    }
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

/// The `writ` a connected program is handed, and what has to happen first.
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
        (None, Some(InstallTarget::ByInstaller)) => PathBuf::from("writ.exe"),
        (None, _) => PathBuf::from(SYSTEM_LINK),
    };
    ClientCommand {
        path,
        needs_install: false,
        needs_move,
    }
}
