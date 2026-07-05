use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub enum OpenTarget {
    Files(Vec<PathBuf>),
    Workspace(PathBuf),
    Stdin { title: Option<String> },
}

#[derive(Debug, PartialEq)]
pub enum ArgError {
    MixedFilesAndWorkspace,
    MultipleWorkspaces,
}

impl std::fmt::Display for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArgError::MixedFilesAndWorkspace => {
                write!(
                    f,
                    "cannot mix files and a workspace directory in one invocation"
                )
            }
            ArgError::MultipleWorkspaces => {
                write!(f, "only one workspace directory may be opened at a time")
            }
        }
    }
}

/// Resolve a set of raw argument paths (from `clap`) against `cwd` to produce
/// an `OpenTarget`. Paths are absolutized but NOT canonicalized — the app
/// performs canonicalization and authorization once the path arrives via the OS
/// open-files mechanism.
///
/// Rules:
/// - A lone `-` means stdin.
/// - A directory argument means open as workspace.
/// - Multiple directories or mixing directories with files is an error.
/// - All relative paths are joined to `cwd`.
pub fn resolve_targets(
    paths: &[OsString],
    cwd: &Path,
    stdin_title: Option<String>,
) -> Result<OpenTarget, ArgError> {
    if paths.is_empty() || (paths.len() == 1 && paths[0] == "-") {
        return Ok(OpenTarget::Stdin { title: stdin_title });
    }

    let mut files: Vec<PathBuf> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();

    for raw in paths {
        let p = Path::new(raw);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };

        if abs.is_dir() {
            dirs.push(abs);
        } else {
            files.push(abs);
        }
    }

    if dirs.len() > 1 {
        return Err(ArgError::MultipleWorkspaces);
    }

    if !dirs.is_empty() && !files.is_empty() {
        return Err(ArgError::MixedFilesAndWorkspace);
    }

    if let Some(dir) = dirs.into_iter().next() {
        return Ok(OpenTarget::Workspace(dir));
    }

    Ok(OpenTarget::Files(files))
}

/// Sanitize a title string to be safe as a filename component.
/// Replaces characters that are illegal or problematic on macOS/Linux/Windows
/// with underscores. Strips leading dots. Trims to 64 bytes.
pub fn sanitize_title(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_start_matches('.');
    let truncated = if trimmed.len() > 64 {
        &trimmed[..64]
    } else {
        trimmed
    };
    if truncated.is_empty() {
        "piped".to_string()
    } else {
        truncated.to_string()
    }
}

/// Returns the path under which stdin content should be written.
///
/// `piped_dir` should be `~/.writ/piped/` (caller supplies it so tests can
/// inject a temp directory).
pub fn stdin_file_path(piped_dir: &Path, id: &str, title: Option<&str>) -> PathBuf {
    let name = match title {
        Some(t) => format!("{}-{}.txt", id, sanitize_title(t)),
        None => format!("{}.txt", id),
    };
    piped_dir.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use tempfile::TempDir;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn empty_args_resolves_to_stdin() {
        let dir = TempDir::new().unwrap();
        let result = resolve_targets(&[], dir.path(), None).unwrap();
        assert_eq!(result, OpenTarget::Stdin { title: None });
    }

    #[test]
    fn dash_arg_resolves_to_stdin() {
        let dir = TempDir::new().unwrap();
        let result = resolve_targets(&[os("-")], dir.path(), Some("my title".to_string())).unwrap();
        assert_eq!(
            result,
            OpenTarget::Stdin {
                title: Some("my title".to_string())
            }
        );
    }

    #[test]
    fn absolute_file_paths_are_preserved() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "x").unwrap();
        let result =
            resolve_targets(&[OsString::from(file.as_os_str())], dir.path(), None).unwrap();
        assert_eq!(result, OpenTarget::Files(vec![file]));
    }

    #[test]
    fn relative_file_paths_are_absolutized_against_cwd() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "x").unwrap();
        let result = resolve_targets(&[os("note.md")], dir.path(), None).unwrap();
        assert_eq!(result, OpenTarget::Files(vec![dir.path().join("note.md")]));
    }

    #[test]
    fn directory_resolves_to_workspace() {
        let dir = TempDir::new().unwrap();
        let result =
            resolve_targets(&[OsString::from(dir.path().as_os_str())], dir.path(), None).unwrap();
        assert_eq!(result, OpenTarget::Workspace(dir.path().to_path_buf()));
    }

    #[test]
    fn dot_resolves_to_workspace() {
        let dir = TempDir::new().unwrap();
        let result = resolve_targets(&[os(".")], dir.path(), None).unwrap();
        assert_eq!(result, OpenTarget::Workspace(dir.path().to_path_buf()));
    }

    #[test]
    fn multiple_directories_is_error() {
        let d1 = TempDir::new().unwrap();
        let d2 = TempDir::new().unwrap();
        let result = resolve_targets(
            &[
                OsString::from(d1.path().as_os_str()),
                OsString::from(d2.path().as_os_str()),
            ],
            d1.path(),
            None,
        );
        assert_eq!(result, Err(ArgError::MultipleWorkspaces));
    }

    #[test]
    fn mixing_files_and_directory_is_error() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("x.txt");
        std::fs::write(&file, "").unwrap();
        let result = resolve_targets(
            &[
                OsString::from(file.as_os_str()),
                OsString::from(dir.path().as_os_str()),
            ],
            dir.path(),
            None,
        );
        assert_eq!(result, Err(ArgError::MixedFilesAndWorkspace));
    }

    #[test]
    fn multiple_files_are_collected() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let result = resolve_targets(
            &[OsString::from(a.as_os_str()), OsString::from(b.as_os_str())],
            dir.path(),
            None,
        )
        .unwrap();
        assert_eq!(result, OpenTarget::Files(vec![a, b]));
    }

    #[test]
    fn sanitize_title_strips_illegal_chars() {
        assert_eq!(sanitize_title("hello/world:test"), "hello_world_test");
    }

    #[test]
    fn sanitize_title_truncates_long_strings() {
        let long = "a".repeat(100);
        assert_eq!(sanitize_title(&long).len(), 64);
    }

    #[test]
    fn sanitize_title_strips_leading_dot() {
        assert_eq!(sanitize_title(".hidden"), "hidden");
    }

    #[test]
    fn sanitize_title_empty_becomes_piped() {
        assert_eq!(sanitize_title(""), "piped");
        assert_eq!(sanitize_title("..."), "piped");
    }

    #[test]
    fn stdin_file_path_without_title() {
        let dir = TempDir::new().unwrap();
        let path = stdin_file_path(dir.path(), "abc123", None);
        assert_eq!(path, dir.path().join("abc123.txt"));
    }

    #[test]
    fn stdin_file_path_with_title() {
        let dir = TempDir::new().unwrap();
        let path = stdin_file_path(dir.path(), "abc123", Some("my notes"));
        assert_eq!(path, dir.path().join("abc123-my notes.txt"));
    }

    #[test]
    fn stdin_file_path_containment() {
        let dir = TempDir::new().unwrap();
        let path = stdin_file_path(dir.path(), "abc123", Some("../escape"));
        assert!(
            path.starts_with(dir.path()),
            "path escaped the piped dir: {path:?}"
        );
    }
}
