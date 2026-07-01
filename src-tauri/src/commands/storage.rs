use crate::state::AppState;
use std::path::Path;
use tauri::State;

#[derive(Debug, serde::Serialize)]
pub struct StorageInfo {
    pub db_path: String,
    pub dir: String,
}

/// IPC: absolute paths to the SQLite database file and its parent directory.
#[tauri::command]
pub fn get_storage_info(state: State<'_, AppState>) -> StorageInfo {
    let dir = state.writ_dir.clone();
    let db_path = dir.join("writ.db");
    StorageInfo {
        db_path: db_path.to_string_lossy().into_owned(),
        dir: dir.to_string_lossy().into_owned(),
    }
}

/// Target platform for the reveal command. Passed explicitly (rather than read
/// from `cfg!` inline) so the argument construction is unit-tested for all three
/// platforms regardless of the host running the tests. Each host builds only its
/// own `HOST_OS`, so the other variants are constructed solely in tests and per
/// the target platform; allow the resulting dead-code flag on this host.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RevealOs {
    Macos,
    Windows,
    Linux,
}

#[cfg(target_os = "macos")]
const HOST_OS: RevealOs = RevealOs::Macos;
#[cfg(target_os = "windows")]
const HOST_OS: RevealOs = RevealOs::Windows;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const HOST_OS: RevealOs = RevealOs::Linux;

/// Program and arguments that reveal `target` in the platform file manager.
///
/// macOS `open -R` and Windows `explorer /select,` select the file inside its
/// folder. Linux has no portable "select" verb, so it opens the parent
/// directory with `xdg-open`.
fn reveal_command(os: RevealOs, target: &Path) -> (String, Vec<String>) {
    match os {
        RevealOs::Macos => (
            "open".into(),
            vec!["-R".into(), target.to_string_lossy().into_owned()],
        ),
        RevealOs::Windows => (
            "explorer".into(),
            vec![format!("/select,{}", target.to_string_lossy())],
        ),
        RevealOs::Linux => {
            let dir = target.parent().unwrap_or(target);
            ("xdg-open".into(), vec![dir.to_string_lossy().into_owned()])
        }
    }
}

/// IPC: reveal the SQLite database in the OS file manager. Falls back to the
/// storage directory when the database file does not exist yet.
#[tauri::command]
pub fn reveal_storage_path(state: State<'_, AppState>) -> Result<(), String> {
    let db_path = state.writ_dir.join("writ.db");
    let target = if db_path.exists() {
        db_path
    } else {
        state.writ_dir.clone()
    };
    let (program, args) = reveal_command(HOST_OS, &target);
    std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            tracing::warn!(error = %e, program, "reveal in file manager failed");
            "Could not open the file manager.".to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_reveal_selects_the_file() {
        let (program, args) = reveal_command(RevealOs::Macos, Path::new("/home/u/.writ/writ.db"));
        assert_eq!(program, "open");
        assert_eq!(args, vec!["-R".to_string(), "/home/u/.writ/writ.db".to_string()]);
    }

    #[test]
    fn windows_reveal_selects_the_file() {
        let (program, args) =
            reveal_command(RevealOs::Windows, Path::new("C:\\Users\\u\\.writ\\writ.db"));
        assert_eq!(program, "explorer");
        assert_eq!(args, vec!["/select,C:\\Users\\u\\.writ\\writ.db".to_string()]);
    }

    #[test]
    fn linux_reveal_opens_the_parent_directory() {
        let (program, args) = reveal_command(RevealOs::Linux, Path::new("/home/u/.writ/writ.db"));
        assert_eq!(program, "xdg-open");
        assert_eq!(args, vec!["/home/u/.writ".to_string()]);
    }

    #[test]
    fn storage_info_serializes_expected_keys() {
        let info = StorageInfo {
            db_path: "/a/writ.db".into(),
            dir: "/a".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("db_path"));
        assert!(json.contains("\"dir\""));
    }
}
