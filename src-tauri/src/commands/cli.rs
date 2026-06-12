use tauri::{AppHandle, Manager};

#[derive(Debug, serde::Serialize)]
pub struct InstallCliResult {
    pub symlink_path: String,
    pub manual_command: String,
}

/// Resolves the path to the bundled `writ` sidecar binary.
///
/// In a released `.app` bundle on macOS the sidecar lives at:
///   `<bundle>/Contents/MacOS/writ-<target-triple>`
/// Tauri resolves this via `app_handle.path().resource_dir()`. In dev mode
/// `resource_dir()` resolves to `src-tauri/` so the binary is on PATH instead.
fn resolve_sidecar_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?;

    let target = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    let triple = match (os, target) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => return None,
    };

    let bin_name = if os == "windows" {
        format!("writ-{}.exe", triple)
    } else {
        format!("writ-{}", triple)
    };

    let candidate = resource_dir.join(&bin_name);
    if candidate.exists() {
        return Some(candidate);
    }

    let macos_candidate = resource_dir.parent()?.join(&bin_name);
    if macos_candidate.exists() {
        return Some(macos_candidate);
    }

    None
}

#[tauri::command]
pub fn install_cli(app: AppHandle) -> Result<InstallCliResult, String> {
    let symlink_path = std::path::PathBuf::from("/usr/local/bin/writ");

    let sidecar = resolve_sidecar_path(&app).ok_or_else(|| {
        "bundled writ binary not found; use cargo tauri build to produce a bundle".to_string()
    })?;

    let manual = format!(
        "ln -sf \"{}\" \"{}\"",
        sidecar.display(),
        symlink_path.display()
    );

    if symlink_path.exists() || symlink_path.symlink_metadata().is_ok() {
        std::fs::remove_file(&symlink_path).map_err(|e| {
            format!(
                "cannot replace existing symlink (try: {}): {e}",
                manual
            )
        })?;
    }

    if let Some(parent) = symlink_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("cannot create {}: {e} (try: {})", parent.display(), manual)
            })?;
        }
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&sidecar, &symlink_path).map_err(|e| {
        format!(
            "cannot create symlink (try: {}): {e}",
            manual
        )
    })?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&sidecar, &symlink_path).map_err(|e| {
        format!(
            "cannot create symlink (try: {}): {e}",
            manual
        )
    })?;

    Ok(InstallCliResult {
        symlink_path: symlink_path.to_string_lossy().into_owned(),
        manual_command: manual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
