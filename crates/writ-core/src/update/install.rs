//! Policy for installs a package manager owns.
//!
//! The updater replaces the running AppImage in place on Linux, with no
//! installer and no elevation. That fails, or writes into files another tool
//! owns, when a package manager put Writ on disk (the AUR package unpacks the
//! AppImage into root-owned `/opt/writ`). The adapter gathers the facts below;
//! this module decides whether Writ may install an update itself or must tell
//! the user to update through their package manager.

/// The file a package's build script writes at the root of the install tree.
/// Its contents name the package manager; only its presence is read.
pub const PACKAGE_MANAGER_MARKER: &str = ".package-manager";

/// How many directories above the executable's own directory are searched for
/// [`PACKAGE_MANAGER_MARKER`]: an AppImage tree keeps the executable at
/// `<root>/usr/bin/`, so two levels reach `<root>`.
pub const MARKER_SEARCH_LEVELS: usize = 2;

/// What the adapter found out about the running install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InstallFacts {
    /// A [`PACKAGE_MANAGER_MARKER`] sits in the install tree.
    pub package_manager_marker: bool,
    /// The updater would overwrite the running file itself, as it does for an
    /// AppImage. False where an installer does the work (macOS, Windows, and
    /// the `.deb` and `.rpm` bundles, which go through dpkg or rpm).
    pub replaces_in_place: bool,
    /// The AppImage runtime set `APPIMAGE` for this launch, so the updater
    /// targets the AppImage file rather than the unpacked executable.
    pub appimage_env: bool,
    /// The user can write the directory that holds the file the updater
    /// replaces.
    pub target_writable: bool,
}

/// Whether updates for this install come from a package manager, so Writ must
/// not download and install one itself.
///
/// A marker always decides. Otherwise only an in-place replacement can fail:
/// without `APPIMAGE` the target is the executable inside an unpacked tree,
/// and a directory the user cannot write cannot take the new file.
pub fn is_managed(facts: &InstallFacts) -> bool {
    facts.package_manager_marker
        || (facts.replaces_in_place && (!facts.appimage_env || !facts.target_writable))
}
