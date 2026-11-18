use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use jotra_core::cli_install::{
    self, carries_cli_signature, may_replace, InstallFacts, InstallTarget, InstalledEntry,
};
use jotra_core::legacy_names;

use crate::commands::offload::{run_detached, ERR_COMMAND_STOPPED};

#[derive(Debug, serde::Serialize)]
pub struct InstallCliResult {
    pub symlink_path: String,
    pub manual_command: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CliStatus {
    pub installed: bool,
    pub path: String,
    /// Whether the app installs the command itself. On Windows the installer
    /// puts it on the PATH, so there is nothing to offer.
    pub can_install: bool,
    /// The app runs from a folder macOS made up for a quarantined download,
    /// and has to be moved before anything may point at it.
    pub needs_move: bool,
}

/// Whether the `jotra` command is present at the install target *and* runnable.
///
/// Follows the link: a symlink left behind by an app bundle that has since been
/// moved or deleted resolves to nothing, and reporting that as installed leaves
/// the user with a shell that answers `command not found`.
fn is_installed(target: &Path) -> bool {
    target.metadata().is_ok()
}

/// Whether the running app is an AppImage: its runtime names the image in
/// `APPIMAGE`, and the folder it runs from is gone once it quits.
fn runs_from_appimage() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some_and(|value| !value.is_empty())
}

/// Where this app puts the `jotra` command ([`cli_install::install_target`]).
pub(crate) fn install_target() -> Option<InstallTarget> {
    let home = dirs::home_dir();
    cli_install::install_target(InstallFacts {
        windows: cfg!(windows),
        in_appimage: runs_from_appimage(),
        home: home.as_deref(),
    })
}

/// Whether the running app has to be moved before the command can point at it.
pub(crate) fn needs_move() -> bool {
    std::env::current_exe().is_ok_and(|exe| cli_install::is_translocated(&exe))
}

/// IPC: [`cli_status_blocking`], off the main thread.
#[tauri::command]
pub async fn cli_status() -> Result<CliStatus, String> {
    run_detached(ERR_COMMAND_STOPPED, || Ok(cli_status_blocking())).await
}

/// Whether the `jotra` command is installed, and where it is or goes.
///
/// On Windows the installer puts the folder holding `jotra.exe` on the PATH, so
/// the command bundled beside the app is the installed one.
pub fn cli_status_blocking() -> CliStatus {
    match install_target() {
        Some(InstallTarget::ByInstaller) => {
            let bundled = bundled_cli_path();
            CliStatus {
                installed: bundled.is_some(),
                path: bundled
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                can_install: false,
                needs_move: false,
            }
        }
        Some(InstallTarget::Link(path) | InstallTarget::Copy(path)) => CliStatus {
            installed: is_installed(&path),
            path: path.display().to_string(),
            can_install: true,
            needs_move: needs_move(),
        },
        None => CliStatus {
            installed: false,
            path: String::new(),
            can_install: false,
            needs_move: false,
        },
    }
}

const SIDECAR_NOT_FOUND: &str = "The jotra command line tool could not be located.";

/// What [`install_cli`] answers on a platform with nowhere to put the command.
const INSTALL_UNSUPPORTED: &str =
    "The jotra command is already on your PATH; Jotra's installer puts it there.";

/// What [`install_cli`] answers for an app macOS runs from a translocated folder.
const MOVE_APP_FIRST: &str =
    "Move Jotra to your Applications folder and open it from there, then install the jotra command.";

/// What [`install_cli`] answers when there is no home folder to copy into.
const NO_HOME: &str = "Jotra could not find your home folder to install the jotra command in.";

/// What [`install_cli`] answers when the path holds another program's `jotra`.
fn another_command_there(path: &Path) -> String {
    format!(
        "Another program named jotra is already at {}, so Jotra left it in place.",
        path.display()
    )
}

/// Name of the bundled CLI sidecar as it sits next to the app executable.
fn sidecar_name() -> &'static str {
    if cfg!(windows) {
        "jotra.exe"
    } else {
        "jotra"
    }
}

/// The sidecar is bundled in the same directory as the running app executable
/// (`Contents/MacOS/` inside a macOS `.app`, next to the `.exe` on Windows).
fn sidecar_candidate(exe_dir: &Path) -> PathBuf {
    exe_dir.join(sidecar_name())
}

/// Name of the same command bundled under its legacy name (ADR-059 decision 1).
pub(crate) fn legacy_sidecar_name() -> String {
    format!(
        "{}{}",
        legacy_names::COMMAND_NAME,
        std::env::consts::EXE_SUFFIX
    )
}

/// The command's legacy name beside the install target, and the bundled
/// command it is pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LegacyCompanion {
    source: PathBuf,
    path: PathBuf,
}

/// The legacy name an install keeps current, when one is already there and
/// may be replaced ([`cli_install::should_refresh_legacy_command`]). It points
/// at the bundled command under that name, or at `sidecar` when the build
/// ships only one.
fn legacy_companion_at(
    target: &InstallTarget,
    sidecar: &Path,
    app_binary_name: Option<&OsStr>,
) -> Option<LegacyCompanion> {
    let path = cli_install::legacy_command_path(target)?;
    if !cli_install::should_refresh_legacy_command(&inspect_entry(&path, app_binary_name)) {
        return None;
    }
    let bundled = sidecar.with_file_name(legacy_sidecar_name());
    let source = if bundled.exists() {
        bundled
    } else {
        sidecar.to_path_buf()
    };
    Some(LegacyCompanion { source, path })
}

/// [`legacy_companion_at`] for the running app.
fn legacy_companion(target: &InstallTarget, sidecar: &Path) -> Option<LegacyCompanion> {
    let app_binary = std::env::current_exe().ok();
    legacy_companion_at(
        target,
        sidecar,
        app_binary.as_deref().and_then(Path::file_name),
    )
}

/// Puts `companion` in place with `place`. The legacy name is kept current
/// alongside the install, so a failure leaves it as it was and the install
/// stands.
fn refresh_companion(
    companion: Option<&LegacyCompanion>,
    place: fn(&Path, &Path) -> std::io::Result<()>,
) {
    let Some(companion) = companion else {
        return;
    };
    if let Err(error) = place(&companion.source, &companion.path) {
        tracing::warn!(
            error = %error,
            path = %companion.path.display(),
            "the command's legacy name was left as it was"
        );
    }
}

/// The bundled `jotra` binary this build ships, when it is there.
///
/// Anchored on the running executable rather than the resource directory: Tauri
/// places sidecars beside the main binary, while `resource_dir()` points at a
/// sibling `Resources/` folder that does not contain it.
pub(crate) fn bundled_cli_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = sidecar_candidate(exe.parent()?);
    candidate.exists().then_some(candidate)
}

/// What sits at `path`, read without following a link.
///
/// A link's target is read as written and resolved against the link's folder;
/// whether the app binary sits beside it is what marks a Linux package's own
/// `jotra`. A file is read to look for the command's signature.
pub(crate) fn inspect_entry(path: &Path, app_binary_name: Option<&OsStr>) -> InstalledEntry {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return InstalledEntry::Absent
        }
        Err(_) => return InstalledEntry::Other,
    };
    if metadata.file_type().is_symlink() {
        let Ok(raw) = std::fs::read_link(path) else {
            return InstalledEntry::Other;
        };
        let target = match path.parent() {
            Some(folder) if raw.is_relative() => folder.join(raw),
            _ => raw,
        };
        let beside_app = app_binary_name.is_some_and(|name| target.with_file_name(name).exists());
        return InstalledEntry::Link { target, beside_app };
    }
    if metadata.is_file() {
        let is_jotra_command = std::fs::read(path).is_ok_and(|bytes| carries_cli_signature(&bytes));
        return InstalledEntry::File { is_jotra_command };
    }
    InstalledEntry::Other
}

/// Refuses, as [`std::io::ErrorKind::AlreadyExists`], to touch `target` when
/// it holds anything but Jotra's own command.
fn refuse_another_command(target: &Path) -> std::io::Result<()> {
    let app_binary = std::env::current_exe().ok();
    let app_binary_name = app_binary.as_deref().and_then(Path::file_name);
    if may_replace(&inspect_entry(target, app_binary_name)) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            another_command_there(target),
        ))
    }
}

/// Create the symlink directly, as the current user.
///
/// Only Jotra's own command is replaced: a link into a Jotra bundle, including
/// one whose bundle is gone, or a copy carrying the command's signature.
/// Anything else at the path is refused with
/// [`std::io::ErrorKind::AlreadyExists`] and left in place.
pub(crate) fn link_directly(sidecar: &Path, target: &Path) -> std::io::Result<()> {
    refuse_another_command(target)?;
    if target.symlink_metadata().is_ok() {
        std::fs::remove_file(target)?;
    }
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(sidecar, target)?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_file(sidecar, target)?;

    Ok(())
}

/// Copies the command to `target`, for an app whose own copy is gone once it
/// quits. The copy lands under a staging name beside the target and is
/// renamed over it, so a failed copy never leaves half a binary on the PATH.
pub(crate) fn copy_command(sidecar: &Path, target: &Path) -> std::io::Result<()> {
    refuse_another_command(target)?;
    let folder = target
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    std::fs::create_dir_all(folder)?;
    let staging = folder.join(format!(".jotra-install-{}", std::process::id()));
    let placed = std::fs::copy(sidecar, &staging)
        .and_then(|_| make_executable(&staging))
        .and_then(|()| std::fs::rename(&staging, target));
    if placed.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    placed
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Wrap `s` as a single POSIX shell word: enclose in single quotes and escape
/// any embedded single quote via the `'\''` idiom. Safe for arbitrary paths,
/// including install locations that contain an apostrophe.
#[cfg(target_os = "macos")]
pub(crate) fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Escape `s` for inclusion in an AppleScript double-quoted string literal:
/// backslash and double quote are the only metacharacters. Backslash is
/// escaped first so the double-quote replacement's backslash is not re-escaped.
#[cfg(target_os = "macos")]
pub(crate) fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The AppleScript that creates the link with an admin prompt.
///
/// Two escaping layers apply: the paths are single-quoted for the inner shell
/// (`shell_single_quote`), then the whole shell command is embedded in an
/// AppleScript double-quoted literal (`applescript_escape`). The sidecar path
/// comes from `current_exe()`, so it can carry an apostrophe when the app is
/// installed under such a directory; both layers keep that from breaking out
/// of the quoting.
///
/// The legacy name in `companion` is linked under the same prompt; its step
/// cannot fail the install.
#[cfg(target_os = "macos")]
fn privileged_link_script(
    sidecar: &Path,
    target: &Path,
    companion: Option<&LegacyCompanion>,
) -> String {
    let parent = target
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut shell_cmd = format!(
        "mkdir -p {} && ln -sf {} {}",
        shell_single_quote(&parent),
        shell_single_quote(&sidecar.to_string_lossy()),
        shell_single_quote(&target.to_string_lossy()),
    );
    if let Some(companion) = companion {
        shell_cmd.push_str(&format!(
            " && {{ ln -sf {} {} || true; }}",
            shell_single_quote(&companion.source.to_string_lossy()),
            shell_single_quote(&companion.path.to_string_lossy()),
        ));
    }
    format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(&shell_cmd)
    )
}

/// Create the symlink after prompting for administrator rights via the native
/// macOS dialog. Used when `/usr/local/bin` is not user-writable.
#[cfg(target_os = "macos")]
fn link_with_privileges(
    sidecar: &Path,
    target: &Path,
    companion: Option<&LegacyCompanion>,
) -> Result<(), String> {
    let script = privileged_link_script(sidecar, target, companion);
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|_| "Could not request permission to install the jotra command.".to_string())?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("User canceled") || stderr.contains("-128") {
        Err("Installation was canceled.".to_string())
    } else {
        Err("Could not install the jotra command.".to_string())
    }
}

/// IPC: [`install_cli_blocking`], off the main thread. The administrator
/// password dialog waits on the person, and the window keeps drawing while it
/// is up (ADR-047).
#[tauri::command]
pub async fn install_cli() -> Result<InstallCliResult, String> {
    run_detached(ERR_COMMAND_STOPPED, install_cli_blocking).await
}

/// Puts the bundled `jotra` command on the PATH: a link in `/usr/local/bin`,
/// asking for an administrator's password when the folder is not writable, or
/// a copy in `~/.local/bin` for an AppImage. A legacy name already beside it
/// is pointed at the same build.
pub fn install_cli_blocking() -> Result<InstallCliResult, String> {
    let target = match install_target() {
        Some(InstallTarget::ByInstaller) => return Err(INSTALL_UNSUPPORTED.to_string()),
        None => return Err(NO_HOME.to_string()),
        Some(target) => target,
    };
    if needs_move() {
        return Err(MOVE_APP_FIRST.to_string());
    }
    let sidecar = bundled_cli_path().ok_or_else(|| SIDECAR_NOT_FOUND.to_string())?;
    let companion = legacy_companion(&target, &sidecar);
    match target {
        InstallTarget::Copy(path) => install_copy(&sidecar, &path, companion.as_ref()),
        InstallTarget::Link(path) => install_link(&sidecar, &path, companion.as_ref()),
        InstallTarget::ByInstaller => Err(INSTALL_UNSUPPORTED.to_string()),
    }
}

fn install_copy(
    sidecar: &Path,
    target: &Path,
    companion: Option<&LegacyCompanion>,
) -> Result<InstallCliResult, String> {
    let manual = format!("cp \"{}\" \"{}\"", sidecar.display(), target.display());
    match copy_command(sidecar, target) {
        Ok(()) => {
            refresh_companion(companion, copy_command);
            Ok(InstallCliResult {
                symlink_path: target.to_string_lossy().into_owned(),
                manual_command: manual,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e.to_string()),
        Err(e) => Err(format!(
            "Could not install the jotra command: {e}. Run this in Terminal: {manual}"
        )),
    }
}

fn install_link(
    sidecar: &Path,
    target: &Path,
    companion: Option<&LegacyCompanion>,
) -> Result<InstallCliResult, String> {
    let manual = format!("ln -sf \"{}\" \"{}\"", sidecar.display(), target.display());

    match link_directly(sidecar, target) {
        Ok(()) => refresh_companion(companion, link_directly),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            #[cfg(target_os = "macos")]
            link_with_privileges(sidecar, target, companion)?;

            #[cfg(not(target_os = "macos"))]
            return Err(format!(
                "Jotra needs permission to install into /usr/local/bin. Run this in Terminal: {manual}"
            ));
        }
        Err(e) => {
            return Err(format!(
                "Could not install the jotra command: {e}. Run this in Terminal: {manual}"
            ));
        }
    }

    Ok(InstallCliResult {
        symlink_path: target.to_string_lossy().into_owned(),
        manual_command: manual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn sidecar_candidate_sits_next_to_the_executable() {
        // Tauri bundles the sidecar in Contents/MacOS, not Resources/, and names
        // it `jotra` (no target triple) — the bug this guards against.
        let dir = Path::new("/Applications/Jotra.app/Contents/MacOS");
        let expected = if cfg!(windows) {
            dir.join("jotra.exe")
        } else {
            PathBuf::from("/Applications/Jotra.app/Contents/MacOS/jotra")
        };
        assert_eq!(sidecar_candidate(dir), expected);
    }

    #[test]
    fn the_status_names_where_the_command_is_or_goes() {
        let status = cli_status_blocking();
        assert_eq!(status.can_install, !cfg!(windows));
        if status.can_install {
            assert!(!status.path.is_empty());
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_leaves_the_install_to_the_installer() {
        assert_eq!(install_cli_blocking().unwrap_err(), INSTALL_UNSUPPORTED);
        assert_eq!(install_target(), Some(InstallTarget::ByInstaller));
    }

    #[test]
    fn user_facing_strings_have_no_developer_internals() {
        let another = another_command_there(Path::new("/usr/local/bin/jotra"));
        for msg in [
            SIDECAR_NOT_FOUND,
            INSTALL_UNSUPPORTED,
            MOVE_APP_FIRST,
            NO_HOME,
            another.as_str(),
            "Installation was canceled.",
        ] {
            let lower = msg.to_ascii_lowercase();
            for token in ["cargo", "build", "tauri", "bundle", "sidecar", "symlink"] {
                assert!(
                    !lower.contains(token),
                    "user message leaks `{token}`: {msg}"
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn privileged_script_links_sidecar_to_target_with_prompt() {
        let script = privileged_link_script(
            Path::new("/Applications/Jotra.app/Contents/MacOS/jotra"),
            Path::new("/usr/local/bin/jotra"),
            None,
        );
        assert!(script.contains("with administrator privileges"));
        assert!(script.contains("/Applications/Jotra.app/Contents/MacOS/jotra"));
        assert!(script.contains("'/usr/local/bin/jotra'"));
        assert!(script.contains("mkdir -p '/usr/local/bin'"));
        assert_eq!(script.matches("ln -sf").count(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn privileged_script_keeps_the_legacy_name_current_without_failing_on_it() {
        let companion = LegacyCompanion {
            source: Path::new("/Applications/Jotra.app/Contents/MacOS").join(legacy_sidecar_name()),
            path: Path::new("/usr/local/bin").join(legacy_names::COMMAND_NAME),
        };
        let script = privileged_link_script(
            Path::new("/Applications/Jotra.app/Contents/MacOS/jotra"),
            Path::new("/usr/local/bin/jotra"),
            Some(&companion),
        );
        let companion_step = format!(
            "{{ ln -sf {} {} || true; }}",
            shell_single_quote(&companion.source.to_string_lossy()),
            shell_single_quote(&companion.path.to_string_lossy()),
        );
        assert!(
            script.contains(&applescript_escape(&companion_step)),
            "{script}"
        );
        assert!(script.contains("'/usr/local/bin/jotra' && {"), "{script}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn privileged_script_quotes_apostrophe_install_path() {
        // The sidecar path comes from current_exe(); an app installed under a
        // directory containing an apostrophe must not break out of the shell
        // single-quoting inside the root `do shell script`.
        let sidecar = Path::new("/Users/o'brien/Apps/Jotra.app/Contents/MacOS/jotra");
        let target = Path::new("/usr/local/bin/jotra");
        let script = privileged_link_script(sidecar, target, None);

        let expected = applescript_escape(&shell_single_quote(&sidecar.to_string_lossy()));
        assert!(
            script.contains(&expected),
            "sidecar path is not fully escaped: {script}"
        );
        // The raw path (quote immediately followed by `brien`) would mean the
        // apostrophe closed the quoting early — it must never appear.
        assert!(
            !script.contains("'/Users/o'brien/Apps"),
            "apostrophe broke out of shell quoting: {script}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shell_single_quote_escapes_embedded_apostrophe() {
        assert_eq!(shell_single_quote("/a/b"), "'/a/b'");
        assert_eq!(shell_single_quote("/o'brien"), "'/o'\\''brien'");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_escape_escapes_backslash_and_quote() {
        assert_eq!(applescript_escape("a\\b"), "a\\\\b");
        assert_eq!(applescript_escape("say \"hi\""), "say \\\"hi\\\"");
    }

    #[test]
    fn is_installed_reflects_presence() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("jotra");
        std::fs::write(&present, "").unwrap();
        assert!(is_installed(&present));
        assert!(!is_installed(&dir.path().join("absent")));
    }

    #[cfg(unix)]
    #[test]
    fn is_installed_rejects_a_link_to_a_removed_app() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Jotra.app/Contents/MacOS");
        std::fs::create_dir_all(&bundle).unwrap();
        let sidecar = bundle.join("jotra");
        std::fs::write(&sidecar, "").unwrap();

        let link = dir.path().join("jotra");
        std::os::unix::fs::symlink(&sidecar, &link).unwrap();
        assert!(is_installed(&link));

        std::fs::remove_file(&sidecar).unwrap();
        assert!(!is_installed(&link));
    }

    /// A sidecar stand-in carrying the command's signature, as a real build does.
    #[cfg(unix)]
    fn signed_sidecar(dir: &Path) -> PathBuf {
        let bundle = dir.join("Jotra.app/Contents/MacOS");
        std::fs::create_dir_all(&bundle).unwrap();
        let sidecar = bundle.join("jotra");
        let mut bytes = b"\x7fELF".to_vec();
        bytes.extend_from_slice(cli_install::CLI_SIGNATURE);
        std::fs::write(&sidecar, bytes).unwrap();
        sidecar
    }

    #[cfg(unix)]
    #[test]
    fn another_programs_jotra_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let target = dir.path().join("bin/jotra");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "#!/bin/sh\necho another jotra\n").unwrap();

        let refused = link_directly(&sidecar, &target).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "#!/bin/sh\necho another jotra\n"
        );
        let refused = copy_command(&sidecar, &target).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);

        let tool = dir.path().join("tools/jotra");
        std::fs::create_dir_all(tool.parent().unwrap()).unwrap();
        std::fs::write(&tool, "").unwrap();
        let link = dir.path().join("bin/linked");
        std::os::unix::fs::symlink(&tool, &link).unwrap();
        assert_eq!(
            link_directly(&sidecar, &link).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read_link(&link).unwrap(), tool);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_into_a_jotra_bundle_is_replaced_even_when_the_bundle_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let target = dir.path().join("bin/jotra");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let old = dir.path().join("Old Jotra.app/Contents/MacOS/jotra");
        std::os::unix::fs::symlink(&old, &target).unwrap();

        link_directly(&sidecar, &target).expect("a stale Jotra link is replaced");
        assert_eq!(std::fs::read_link(&target).unwrap(), sidecar);
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_copy_is_executable_and_replaces_only_a_jotra_copy() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let target = dir.path().join("home/.local/bin/jotra");

        copy_command(&sidecar, &target).expect("copied");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            std::fs::read(&sidecar).unwrap()
        );
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the copy is not executable");

        // An update copies over the copy an earlier version made.
        copy_command(&sidecar, &target).expect("a Jotra copy is replaced");
        let left: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left, ["jotra"], "the staging copy was left behind");
    }

    /// The same build bundled under the legacy name beside `sidecar`.
    #[cfg(unix)]
    fn signed_legacy_sidecar(sidecar: &Path) -> PathBuf {
        let legacy = sidecar.with_file_name(legacy_sidecar_name());
        let mut bytes = std::fs::read(sidecar).unwrap();
        bytes.extend_from_slice(b" legacy");
        std::fs::write(&legacy, bytes).unwrap();
        legacy
    }

    #[test]
    fn the_legacy_sidecar_carries_the_legacy_name_and_platform_suffix() {
        assert_eq!(
            legacy_sidecar_name(),
            format!(
                "{}{}",
                legacy_names::COMMAND_NAME,
                std::env::consts::EXE_SUFFIX
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_install_points_an_earlier_legacy_name_at_the_current_build() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let bundled_legacy = signed_legacy_sidecar(&sidecar);
        let target = dir.path().join("bin/jotra");
        let legacy_path = dir.path().join("bin").join(legacy_names::COMMAND_NAME);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let old = dir
            .path()
            .join("Old.app/Contents/MacOS")
            .join(legacy_names::COMMAND_NAME);
        std::os::unix::fs::symlink(&old, &legacy_path).unwrap();

        let install = InstallTarget::Link(target.clone());
        let companion = legacy_companion_at(&install, &sidecar, None).expect("kept current");
        assert_eq!(
            companion,
            LegacyCompanion {
                source: bundled_legacy.clone(),
                path: legacy_path.clone(),
            }
        );

        let installed = install_link(&sidecar, &target, Some(&companion)).expect("installed");
        assert_eq!(installed.symlink_path, target.to_string_lossy());
        assert_eq!(std::fs::read_link(&target).unwrap(), sidecar);
        assert_eq!(std::fs::read_link(&legacy_path).unwrap(), bundled_legacy);
    }

    #[cfg(unix)]
    #[test]
    fn an_install_never_adds_the_legacy_name() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        signed_legacy_sidecar(&sidecar);
        let target = dir.path().join("bin/jotra");
        let install = InstallTarget::Link(target.clone());

        let companion = legacy_companion_at(&install, &sidecar, None);
        assert_eq!(companion, None);
        install_link(&sidecar, &target, companion.as_ref()).expect("installed");
        assert!(dir
            .path()
            .join("bin")
            .join(legacy_names::COMMAND_NAME)
            .symlink_metadata()
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn another_programs_file_under_the_legacy_name_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let legacy_path = dir.path().join("bin").join(legacy_names::COMMAND_NAME);
        std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        std::fs::write(&legacy_path, "#!/bin/sh\necho another program\n").unwrap();

        let install = InstallTarget::Link(dir.path().join("bin/jotra"));
        assert_eq!(legacy_companion_at(&install, &sidecar, None), None);
        assert_eq!(
            std::fs::read_to_string(&legacy_path).unwrap(),
            "#!/bin/sh\necho another program\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_build_with_one_name_points_the_legacy_name_at_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let legacy_path = dir.path().join("bin").join(legacy_names::COMMAND_NAME);
        std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        std::fs::copy(&sidecar, &legacy_path).unwrap();

        let install = InstallTarget::Link(dir.path().join("bin/jotra"));
        let companion = legacy_companion_at(&install, &sidecar, None).expect("kept current");
        assert_eq!(companion.source, sidecar);
        assert_eq!(companion.path, legacy_path);
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_copy_refreshes_an_earlier_legacy_copy() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let bundled_legacy = signed_legacy_sidecar(&sidecar);
        let target = dir.path().join("home/.local/bin/jotra");
        let legacy_path = target.with_file_name(legacy_names::COMMAND_NAME);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut earlier = b"older build ".to_vec();
        earlier.extend_from_slice(cli_install::CLI_SIGNATURE);
        std::fs::write(&legacy_path, earlier).unwrap();

        let install = InstallTarget::Copy(target.clone());
        let companion = legacy_companion_at(&install, &sidecar, None);
        install_copy(&sidecar, &target, companion.as_ref()).expect("installed");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            std::fs::read(&sidecar).unwrap()
        );
        assert_eq!(
            std::fs::read(&legacy_path).unwrap(),
            std::fs::read(&bundled_legacy).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_companion_that_cannot_be_placed_leaves_the_install_standing() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = signed_sidecar(dir.path());
        let target = dir.path().join("bin/jotra");
        let blocked = LegacyCompanion {
            source: sidecar.clone(),
            path: dir.path().join("bin/folder"),
        };
        std::fs::create_dir_all(&blocked.path).unwrap();

        install_link(&sidecar, &target, Some(&blocked)).expect("installed");
        assert_eq!(std::fs::read_link(&target).unwrap(), sidecar);
        assert!(blocked.path.is_dir());
    }

    // The MSI puts the folder holding jotra.exe on the PATH through a WiX
    // fragment. Only a release build compiles it, so its wiring is checked
    // here: the component the config references is the one the fragment
    // defines, under the install folder, setting PATH machine-wide.
    #[test]
    fn the_windows_installer_puts_the_command_on_the_path() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
            .expect("read tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(&raw).expect("parse tauri.conf.json");
        let wix = &conf["bundle"]["windows"]["wix"];
        let fragment_path = wix["fragmentPaths"][0].as_str().expect("a WiX fragment");
        let component = wix["componentRefs"][0].as_str().expect("a component ref");
        let fragment =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(fragment_path))
                .expect("read the fragment");

        assert!(fragment.contains(&format!("<Component Id=\"{component}\"")));
        assert!(fragment.contains("<DirectoryRef Id=\"INSTALLDIR\">"));
        assert!(fragment.contains("Name=\"PATH\""));
        assert!(fragment.contains("Value=\"[INSTALLDIR]\""));
        assert!(fragment.contains("System=\"yes\""));
        assert!(fragment.contains("Part=\"last\""));
        assert!(fragment.contains("KeyPath=\"yes\""));
        assert!(
            !fragment.contains("Guid=\"*\""),
            "a component without a file needs a fixed GUID"
        );
    }

    #[test]
    fn install_cli_result_serializes() {
        let r = InstallCliResult {
            symlink_path: "/usr/local/bin/jotra".to_string(),
            manual_command: "ln -sf /path/to/jotra /usr/local/bin/jotra".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("symlink_path"));
        assert!(json.contains("manual_command"));
    }
}
