//! Whether a client's entry for the server launches a build of the `jotra`
//! command (`jotra_core::mcp_clients`, ADR-059 decision 7).
//!
//! An entry made before the rename names the command by an old path: a link
//! into the old app bundle on macOS, which the bundle's compatibility link
//! still resolves, or the old install folder on Windows. Reading the file the
//! path resolves to, whatever it went through, is what tells the command
//! apart from another program.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use jotra_core::cli_install::carries_cli_signature;
use jotra_core::mcp_clients::{registration_status, RegistrationStatus};

/// The suffix Windows finds a bare command name under.
const WINDOWS_SUFFIX: &str = "exe";

/// The file `command` names, as a client launching it finds it.
///
/// A path with a folder in it is taken as written, links followed when it is
/// read. A bare name is looked up in each folder of `search_path`, the
/// client's `PATH`, in order, with the Windows suffix tried after the name as
/// written when `windows` is set. `None` when nothing is there.
pub fn resolve_command(
    command: &Path,
    search_path: Option<&OsStr>,
    windows: bool,
) -> Option<PathBuf> {
    if command.as_os_str().is_empty() {
        return None;
    }
    if command.components().count() > 1 || command.is_absolute() {
        return command.is_file().then(|| command.to_path_buf());
    }
    let mut names = vec![command.to_path_buf()];
    if windows && command.extension().is_none() {
        names.push(command.with_extension(WINDOWS_SUFFIX));
    }
    std::env::split_paths(search_path?)
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Whether `command` resolves to a file carrying the command's signature.
pub fn command_carries_signature(command: &Path) -> bool {
    std::fs::read(command).is_ok_and(|bytes| carries_cli_signature(&bytes))
}

/// What a client's entry named `name`, launching `command`, reads as.
/// `search_path` and `windows` are as [`resolve_command`] takes them.
pub fn read_registration_status(
    name: &str,
    command: &Path,
    search_path: Option<&OsStr>,
    windows: bool,
) -> RegistrationStatus {
    let is_jotra = resolve_command(command, search_path, windows)
        .is_some_and(|resolved| command_carries_signature(&resolved));
    registration_status(name, is_jotra)
}
