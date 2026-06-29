#[derive(Debug, serde::Serialize)]
pub struct InstallCliResult {
    pub symlink_path: String,
    pub manual_command: String,
}

const SIDECAR_NOT_FOUND: &str = "The writ command line tool could not be located.";

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
fn sidecar_candidate(exe_dir: &std::path::Path) -> std::path::PathBuf {
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

#[tauri::command]
pub fn install_cli() -> Result<InstallCliResult, String> {
    let symlink_path = std::path::PathBuf::from("/usr/local/bin/writ");

    let sidecar = resolve_sidecar_path().ok_or_else(|| SIDECAR_NOT_FOUND.to_string())?;

    let manual = format!(
        "ln -sf \"{}\" \"{}\"",
        sidecar.display(),
        symlink_path.display()
    );

    if symlink_path.exists() || symlink_path.symlink_metadata().is_ok() {
        std::fs::remove_file(&symlink_path)
            .map_err(|e| format!("Could not replace the existing link: {e}. Run this in Terminal: {manual}"))?;
    }

    if let Some(parent) = symlink_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("Could not create {}: {e}. Run this in Terminal: {manual}", parent.display())
            })?;
        }
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&sidecar, &symlink_path)
        .map_err(|e| format!("Could not link writ into /usr/local/bin: {e}. Run this in Terminal: {manual}"))?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&sidecar, &symlink_path)
        .map_err(|e| format!("Could not link writ: {e}. Run this in Terminal: {manual}"))?;

    Ok(InstallCliResult {
        symlink_path: symlink_path.to_string_lossy().into_owned(),
        manual_command: manual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

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
    fn not_found_message_has_no_developer_internals() {
        let lower = SIDECAR_NOT_FOUND.to_ascii_lowercase();
        for token in ["cargo", "build", "tauri", "bundle", "sidecar", "binary"] {
            assert!(!lower.contains(token), "user message leaks `{token}`: {SIDECAR_NOT_FOUND}");
        }
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
