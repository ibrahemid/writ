use std::path::Path;

#[derive(Debug, serde::Serialize)]
pub struct InstallCliResult {
    pub symlink_path: String,
    pub manual_command: String,
}

const SIDECAR_NOT_FOUND: &str = "The writ command line tool could not be located.";
const INSTALL_TARGET: &str = "/usr/local/bin/writ";

/// Name of the bundled CLI sidecar as it sits next to the app executable.
fn sidecar_name() -> &'static str {
    if cfg!(windows) {
        "writ.exe"
    } else {
        "writ"
    }
}

/// The sidecar is bundled in the same directory as the running app executable
/// (`Contents/MacOS/` inside a macOS `.app`, next to the `.exe` on Windows).
fn sidecar_candidate(exe_dir: &Path) -> std::path::PathBuf {
    exe_dir.join(sidecar_name())
}

/// Resolves the path to the bundled `writ` sidecar binary.
///
/// Anchored on the running executable rather than the resource directory: Tauri
/// places sidecars beside the main binary, while `resource_dir()` points at a
/// sibling `Resources/` folder that does not contain it.
fn resolve_sidecar_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = sidecar_candidate(exe.parent()?);
    candidate.exists().then_some(candidate)
}

/// Create the symlink directly, as the current user.
fn link_directly(sidecar: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() || target.symlink_metadata().is_ok() {
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

/// The AppleScript that creates the link with an admin prompt. Paths are
/// single-quoted for the inner shell; app-bundle paths never contain a single
/// quote, so no further escaping is needed.
#[cfg(target_os = "macos")]
fn privileged_link_script(sidecar: &Path, target: &Path) -> String {
    let parent = target
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "do shell script \"mkdir -p '{}' && ln -sf '{}' '{}'\" with administrator privileges",
        parent,
        sidecar.display(),
        target.display()
    )
}

/// Create the symlink after prompting for administrator rights via the native
/// macOS dialog. Used when `/usr/local/bin` is not user-writable.
#[cfg(target_os = "macos")]
fn link_with_privileges(sidecar: &Path, target: &Path) -> Result<(), String> {
    let script = privileged_link_script(sidecar, target);
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|_| "Could not request permission to install the writ command.".to_string())?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("User canceled") || stderr.contains("-128") {
        Err("Installation was cancelled.".to_string())
    } else {
        Err("Could not install the writ command.".to_string())
    }
}

#[tauri::command]
pub fn install_cli() -> Result<InstallCliResult, String> {
    let target = std::path::PathBuf::from(INSTALL_TARGET);
    let sidecar = resolve_sidecar_path().ok_or_else(|| SIDECAR_NOT_FOUND.to_string())?;
    let manual = format!(
        "ln -sf \"{}\" \"{}\"",
        sidecar.display(),
        target.display()
    );

    match link_directly(&sidecar, &target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            #[cfg(target_os = "macos")]
            link_with_privileges(&sidecar, &target)?;

            #[cfg(not(target_os = "macos"))]
            return Err(format!(
                "Writ needs permission to install into /usr/local/bin. Run this in Terminal: {manual}"
            ));
        }
        Err(e) => {
            return Err(format!(
                "Could not install the writ command: {e}. Run this in Terminal: {manual}"
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
        // it `writ` (no target triple) — the bug this guards against.
        let dir = Path::new("/Applications/Writ.app/Contents/MacOS");
        let expected = if cfg!(windows) {
            dir.join("writ.exe")
        } else {
            PathBuf::from("/Applications/Writ.app/Contents/MacOS/writ")
        };
        assert_eq!(sidecar_candidate(dir), expected);
    }

    #[test]
    fn user_facing_strings_have_no_developer_internals() {
        for msg in [SIDECAR_NOT_FOUND, "Installation was cancelled."] {
            let lower = msg.to_ascii_lowercase();
            for token in ["cargo", "build", "tauri", "bundle", "sidecar", "symlink"] {
                assert!(!lower.contains(token), "user message leaks `{token}`: {msg}");
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn privileged_script_links_sidecar_to_target_with_prompt() {
        let script = privileged_link_script(
            Path::new("/Applications/Writ.app/Contents/MacOS/writ"),
            Path::new("/usr/local/bin/writ"),
        );
        assert!(script.contains("with administrator privileges"));
        assert!(script.contains("/Applications/Writ.app/Contents/MacOS/writ"));
        assert!(script.contains("'/usr/local/bin/writ'"));
        assert!(script.contains("mkdir -p '/usr/local/bin'"));
    }

    #[test]
    fn install_cli_result_serializes() {
        let r = InstallCliResult {
            symlink_path: "/usr/local/bin/writ".to_string(),
            manual_command: "ln -sf /path/to/writ /usr/local/bin/writ".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("symlink_path"));
        assert!(json.contains("manual_command"));
    }
}
