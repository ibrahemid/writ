//! Argument resolution, note verbs and launch plumbing for the `writ` command.
//!
//! The crate links `writ-core` and `writ-storage` and no Tauri, so it builds
//! and runs without the editor while answering from the same policy and the
//! same note index the app uses (`docs/ARCHITECTURE.md`, ADR-017). The verbs in
//! [`verbs`] read that index; the launch path below shells out to the app.
//!
//! The notes folder is resolved by `writ_core::notes::resolve_notes_root_from`,
//! the function the app resolves it with. The launch path keeps its own
//! Finder-style dedupe, which mirrors `writ_core::notes` rather than calling it;
//! `writ_core::notes::sanitize_title` is the authority on what a title may
//! become as a filename, the sanitiser here is the conservative subset the CLI
//! has always applied, and the app re-sanitises anything it opens.

/// The note verbs: `links`, `backlinks`, `properties`, `tags`, `new`, `rename`
/// and `trash`.
pub mod verbs;

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use writ_core::notes::{resolve_notes_root_from, NotesRootError, NotesRootSources};

/// Environment variable naming the Writ application binary to launch.
/// Read on Linux and Windows; macOS resolves the app by bundle identifier.
pub const GUI_BIN_ENV: &str = "WRIT_GUI_BIN";

/// Name of the Writ application binary as it ships beside the CLI: `usr/bin/`
/// in the .deb and the AppImage payload, the install directory in the MSI.
#[cfg(windows)]
pub const GUI_BINARY_NAME: &str = "writ-tauri.exe";
#[cfg(not(windows))]
pub const GUI_BINARY_NAME: &str = "writ-tauri";

#[derive(Debug, PartialEq)]
pub enum OpenTarget {
    Files(Vec<PathBuf>),
    Workspace(PathBuf),
    Stdin { title: Option<String> },
}

/// Where the CLI should send its payload.
#[derive(Debug, PartialEq)]
pub enum GuiLaunch {
    /// Run this binary directly, with the paths as arguments.
    Binary(PathBuf),
    /// No Writ binary could be located; hand the paths to the desktop default
    /// handler, which may open them in another application.
    OsDefault,
}

/// What an invocation carrying no path arguments should do.
#[derive(Debug, PartialEq)]
pub enum NoPathAction {
    /// Read the piped payload from stdin.
    ReadStdin,
    /// Open Writ with no document.
    LaunchApp,
    /// `-` was requested but stdin is a terminal.
    StdinIsTerminal,
}

/// Locate the Writ application binary on Linux and Windows.
///
/// Order: `WRIT_GUI_BIN` when it points at a file, then a `writ-tauri` sibling
/// of the running CLI, then the desktop default handler. Both the .deb and the
/// AppImage payload place `usr/bin/writ` next to `usr/bin/writ-tauri`, and the
/// MSI installs `writ.exe` next to `writ-tauri.exe`, so the sibling lookup
/// covers every shipped layout.
///
/// A candidate that is the running CLI itself is skipped: launching it would
/// re-enter the CLI, which would launch it again, and nothing would ever open.
pub fn resolve_gui_binary(env_override: Option<&Path>, current_exe: Option<&Path>) -> GuiLaunch {
    if let Some(candidate) = env_override {
        if candidate.is_file() && !is_current_exe(candidate, current_exe) {
            return GuiLaunch::Binary(candidate.to_path_buf());
        }
    }

    if let Some(sibling) = current_exe
        .and_then(Path::parent)
        .map(|dir| dir.join(GUI_BINARY_NAME))
    {
        if sibling.is_file() && !is_current_exe(&sibling, current_exe) {
            return GuiLaunch::Binary(sibling);
        }
    }

    GuiLaunch::OsDefault
}

/// Whether `candidate` is the running CLI.
///
/// Both paths are canonicalized so a symlink to the CLI is recognized as the
/// CLI. A path that cannot be canonicalized is compared literally.
fn is_current_exe(candidate: &Path, current_exe: Option<&Path>) -> bool {
    let Some(current_exe) = current_exe else {
        return false;
    };
    match (candidate.canonicalize(), current_exe.canonicalize()) {
        (Ok(candidate), Ok(current_exe)) => candidate == current_exe,
        _ => candidate == current_exe,
    }
}

/// Whether the application process failed to start.
///
/// `exit_success` is `None` while the process is still running and
/// `Some(succeeded)` once it has exited. Invoking Writ while it is already open
/// forwards the arguments to the running instance and exits 0, so only a
/// nonzero exit within the startup window means nothing was opened.
pub fn is_failed_startup(exit_success: Option<bool>) -> bool {
    exit_success == Some(false)
}

/// Whether a piped payload carries nothing worth opening.
///
/// Editor hooks pipe the output of a filter that matches nothing on most
/// events, so an empty payload is dropped rather than opened as a blank buffer.
pub fn is_empty_payload(content: &str) -> bool {
    content.trim().is_empty()
}

/// Decide what to do when no paths were given.
///
/// A pipe is read as stdin content. On a terminal, an explicit `-` is an error
/// because there is nothing to read, while a bare `writ` opens the app.
pub fn no_path_action(stdin_is_pipe: bool, dash_given: bool) -> NoPathAction {
    if stdin_is_pipe {
        NoPathAction::ReadStdin
    } else if dash_given {
        NoPathAction::StdinIsTerminal
    } else {
        NoPathAction::LaunchApp
    }
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

/// Folder name of the default notes folder, under the user's home folder.
/// Mirrors `writ_core::notes::DEFAULT_NOTES_FOLDER`.
pub const NOTES_FOLDER: &str = "Writ";

/// Extension every note carries.
const NOTE_EXTENSION: &str = "md";

/// Reads `[notes] root` out of `<writ_dir>/config.toml`.
///
/// Returns `None` when the file is absent, unreadable, not valid TOML, or the
/// key is unset or blank. Deliberately tolerant: piping text into Writ must
/// not fail because the app has never run or because somebody hand-edited the
/// config into something the CLI cannot parse.
pub fn read_notes_root_from_config(writ_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(writ_dir.join("config.toml")).ok()?;
    let document: toml::Table = toml::from_str(&text).ok()?;
    let root = document.get("notes")?.get("root")?.as_str()?.trim();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

/// Resolves the notes folder the way the app does: `WRIT_NOTES_DIR`, then
/// `config.toml`, then the default, and a source that resolves to nothing
/// usable falls through to the next rather than ending the run.
///
/// Expansion of a leading `~/` and the refusal of a relative path live in
/// [`writ_core::notes::resolve_notes_root_from`], which is asked about one
/// source at a time so the fall-through here is the one
/// `resolve_and_create_notes_root` takes in the app. A `notes.root` of `Notes`
/// therefore names the default folder in both, rather than working in the app
/// and refusing here. Only the default failing is fatal, which is the app's
/// rule too.
///
/// The app additionally skips a folder it cannot create and one holding Writ's
/// own data, both of which it learns by creating the folder. Nothing here
/// creates anything, so a configured folder that is absent is named, and the
/// verb that needs it says it could not be read.
///
/// `data_dir` is the data folder, passed only when `WRIT_DATA_DIR` is set: a
/// dev or recording instance keeps its notes beside its own database rather
/// than writing into the folder the user reads.
pub fn resolve_notes_dir(
    env_override: Option<&str>,
    configured: Option<&Path>,
    data_dir: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, NotesRootError> {
    let configured = configured.and_then(Path::to_str);
    for candidate in [env_override, configured] {
        let Some(chosen) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        if let Ok(root) = resolve_notes_root_from(NotesRootSources {
            env_override: Some(chosen),
            configured: None,
            data_dir: None,
            home,
        }) {
            return Ok(root);
        }
    }

    resolve_notes_root_from(NotesRootSources {
        env_override: None,
        configured: None,
        data_dir,
        home,
    })
}

/// The path a piped payload is written to: `<notes>/<title-or-date>.md`.
///
/// A payload with no title is named for the local calendar day, which is what
/// the app names an untitled note. The name dedupes Finder-style against what
/// the folder already holds, so piping twice on one day produces two notes
/// rather than one overwriting the other.
pub fn piped_note_path(
    notes_dir: &Path,
    title: Option<&str>,
    now: chrono::DateTime<chrono::Local>,
) -> PathBuf {
    let stem = title
        .map(sanitize_title)
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());
    notes_dir.join(dedupe_file_name(
        &stem,
        NOTE_EXTENSION,
        &taken_names(notes_dir),
    ))
}

/// Finder-style dedupe: `stem.md`, `stem 2.md`, `stem 3.md`, and so on.
///
/// `taken` holds lowercased file names including the extension, so the check
/// is case-insensitive the way APFS and NTFS are. Mirrors
/// `writ_core::notes::dedupe_file_name`.
fn dedupe_file_name(stem: &str, extension: &str, taken: &HashSet<String>) -> String {
    let candidate = format!("{stem}.{extension}");
    if !taken.contains(&candidate.to_lowercase()) {
        return candidate;
    }
    let mut counter: u64 = 2;
    loop {
        let candidate = format!("{stem} {counter}.{extension}");
        if !taken.contains(&candidate.to_lowercase()) {
            return candidate;
        }
        counter += 1;
    }
}

/// The names `dir` already holds, lowercased the way the dedupe compares them.
///
/// A folder that cannot be listed yields no names rather than an error: the
/// dedupe is then only less exact, and refusing to name a note because its
/// folder could not be read would lose the text stdin is holding.
fn taken_names(dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect()
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

    fn noon() -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2026, 8, 29, 12, 0, 0)
            .unwrap()
    }

    #[test]
    fn piped_note_path_uses_the_configured_notes_root() {
        let dir = TempDir::new().unwrap();
        let configured = dir.path().join("Elsewhere");
        std::fs::write(
            dir.path().join("config.toml"),
            format!("[notes]\nroot = \"{}\"\n", configured.display()),
        )
        .unwrap();

        let root = read_notes_root_from_config(dir.path()).expect("configured root");
        let notes = resolve_notes_dir(None, Some(&root), None, Some(dir.path()))
            .expect("a configured absolute root resolves");
        assert_eq!(notes, configured);
        assert_eq!(
            piped_note_path(&notes, Some("my notes"), noon()),
            configured.join("my notes.md")
        );
    }

    #[test]
    fn piped_note_path_falls_back_to_home_writ() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read_notes_root_from_config(dir.path()), None);

        let notes =
            resolve_notes_dir(None, None, None, Some(dir.path())).expect("the default resolves");
        assert_eq!(notes, dir.path().join("Writ"));
        assert_eq!(
            piped_note_path(&notes, None, noon()),
            notes.join("2026-08-29.md")
        );
    }

    #[test]
    fn piped_note_path_dedupes() {
        let dir = TempDir::new().unwrap();
        touch(&dir.path().join("my notes.md"));
        touch(&dir.path().join("my notes 2.md"));

        assert_eq!(
            piped_note_path(dir.path(), Some("my notes"), noon()),
            dir.path().join("my notes 3.md")
        );
    }

    #[test]
    fn piped_note_is_markdown_not_txt() {
        let dir = TempDir::new().unwrap();
        let path = piped_note_path(dir.path(), Some("release notes"), noon());
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("md"));
        assert!(
            path.starts_with(dir.path()),
            "the note escaped the notes folder: {path:?}"
        );
    }

    #[test]
    fn a_title_that_would_escape_the_folder_cannot() {
        let dir = TempDir::new().unwrap();
        let path = piped_note_path(dir.path(), Some("../escape"), noon());
        assert!(
            path.starts_with(dir.path()),
            "the note escaped the notes folder: {path:?}"
        );
    }

    #[test]
    fn the_env_override_wins_over_the_config() {
        let dir = TempDir::new().unwrap();
        let configured = dir.path().join("Configured");
        let overridden = dir.path().join("Overridden");
        let notes = resolve_notes_dir(
            overridden.to_str(),
            Some(&configured),
            None,
            Some(dir.path()),
        )
        .expect("an absolute override resolves");
        assert_eq!(notes, overridden);
    }

    #[test]
    fn a_data_folder_override_keeps_its_notes_beside_itself() {
        let dir = TempDir::new().unwrap();
        let data = dir.path().join("instance");
        let notes = resolve_notes_dir(None, None, Some(&data), Some(dir.path()))
            .expect("a data folder override resolves");
        assert_eq!(notes, data.join("Writ"));
    }

    #[test]
    fn a_leading_tilde_expands_against_home() {
        let dir = TempDir::new().unwrap();
        let notes = resolve_notes_dir(Some("~/Notes"), None, None, Some(dir.path()))
            .expect("a tilde path resolves");
        assert_eq!(notes, dir.path().join("Notes"));
    }

    #[test]
    fn a_relative_source_falls_through_to_the_next_one() {
        // A relative folder would follow whichever directory the process
        // started in, so neither surface uses it. The app skips the source and
        // tries the next; refusing outright here would name a different folder
        // than the app for the same config.
        let dir = TempDir::new().unwrap();
        let configured = dir.path().join("Configured");
        for relative in ["Notes", "./Notes", "../Notes"] {
            assert_eq!(
                resolve_notes_dir(Some(relative), Some(&configured), None, Some(dir.path()))
                    .expect("the config resolves"),
                configured,
                "{relative} did not fall through to the config"
            );
            assert_eq!(
                resolve_notes_dir(Some(relative), None, None, Some(dir.path()))
                    .expect("the default resolves"),
                dir.path().join("Writ"),
                "{relative} did not fall through to the default"
            );
        }
    }

    #[test]
    fn a_relative_config_falls_through_to_the_default() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            resolve_notes_dir(None, Some(Path::new("Notes")), None, Some(dir.path()))
                .expect("the default resolves"),
            dir.path().join("Writ")
        );
    }

    #[test]
    fn only_the_default_failing_ends_the_run() {
        let refusal = resolve_notes_dir(Some("Notes"), None, None, None)
            .expect_err("no home and no usable source is fatal");
        assert!(matches!(refusal, NotesRootError::NoHome), "{refusal:?}");
    }

    fn touch(path: &Path) {
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn env_override_is_used_when_it_points_at_a_file() {
        let dir = TempDir::new().unwrap();
        let gui = dir.path().join("Writ-AppRun");
        touch(&gui);
        assert_eq!(
            resolve_gui_binary(Some(&gui), None),
            GuiLaunch::Binary(gui.clone())
        );
    }

    #[test]
    fn env_override_wins_over_sibling() {
        let over = TempDir::new().unwrap();
        let bin = TempDir::new().unwrap();
        let gui = over.path().join("Writ-AppRun");
        touch(&gui);
        touch(&bin.path().join(GUI_BINARY_NAME));
        let cli = bin.path().join("writ");
        touch(&cli);
        assert_eq!(
            resolve_gui_binary(Some(&gui), Some(&cli)),
            GuiLaunch::Binary(gui)
        );
    }

    #[test]
    fn missing_env_override_falls_through_to_sibling() {
        let dir = TempDir::new().unwrap();
        let sibling = dir.path().join(GUI_BINARY_NAME);
        touch(&sibling);
        let cli = dir.path().join("writ");
        touch(&cli);
        let absent = dir.path().join("not-here");
        assert_eq!(
            resolve_gui_binary(Some(&absent), Some(&cli)),
            GuiLaunch::Binary(sibling)
        );
    }

    #[test]
    fn directory_env_override_falls_through_to_sibling() {
        let dir = TempDir::new().unwrap();
        let sibling = dir.path().join(GUI_BINARY_NAME);
        touch(&sibling);
        let cli = dir.path().join("writ");
        touch(&cli);
        assert_eq!(
            resolve_gui_binary(Some(dir.path()), Some(&cli)),
            GuiLaunch::Binary(sibling)
        );
    }

    #[test]
    fn sibling_is_resolved_without_an_override() {
        let dir = TempDir::new().unwrap();
        let sibling = dir.path().join(GUI_BINARY_NAME);
        touch(&sibling);
        let cli = dir.path().join("writ");
        touch(&cli);
        assert_eq!(
            resolve_gui_binary(None, Some(&cli)),
            GuiLaunch::Binary(sibling)
        );
    }

    #[test]
    fn missing_sibling_falls_back_to_os_default() {
        let dir = TempDir::new().unwrap();
        let cli = dir.path().join("writ");
        touch(&cli);
        assert_eq!(resolve_gui_binary(None, Some(&cli)), GuiLaunch::OsDefault);
    }

    #[test]
    fn unknown_current_exe_falls_back_to_os_default() {
        assert_eq!(resolve_gui_binary(None, None), GuiLaunch::OsDefault);
    }

    #[test]
    fn env_override_pointing_at_the_cli_itself_is_skipped() {
        let dir = TempDir::new().unwrap();
        let cli = dir.path().join("writ");
        touch(&cli);
        assert_eq!(
            resolve_gui_binary(Some(&cli), Some(&cli)),
            GuiLaunch::OsDefault
        );
    }

    #[test]
    fn env_override_pointing_at_the_cli_itself_falls_through_to_sibling() {
        let dir = TempDir::new().unwrap();
        let cli = dir.path().join("writ");
        touch(&cli);
        let sibling = dir.path().join(GUI_BINARY_NAME);
        touch(&sibling);
        assert_eq!(
            resolve_gui_binary(Some(&cli), Some(&cli)),
            GuiLaunch::Binary(sibling)
        );
    }

    #[cfg(unix)]
    #[test]
    fn env_override_symlinked_to_the_cli_is_skipped() {
        let dir = TempDir::new().unwrap();
        let cli = dir.path().join("writ");
        touch(&cli);
        let link = dir.path().join("writ-link");
        std::os::unix::fs::symlink(&cli, &link).unwrap();
        assert_eq!(
            resolve_gui_binary(Some(&link), Some(&cli)),
            GuiLaunch::OsDefault
        );
    }

    #[test]
    fn sibling_that_is_the_cli_itself_is_skipped() {
        let dir = TempDir::new().unwrap();
        let cli = dir.path().join(GUI_BINARY_NAME);
        touch(&cli);
        assert_eq!(resolve_gui_binary(None, Some(&cli)), GuiLaunch::OsDefault);
    }

    #[test]
    fn a_running_app_is_not_a_failed_startup() {
        assert!(!is_failed_startup(None));
    }

    #[test]
    fn an_app_that_forwarded_its_arguments_and_exited_is_not_a_failed_startup() {
        assert!(!is_failed_startup(Some(true)));
    }

    #[test]
    fn an_app_that_exited_nonzero_is_a_failed_startup() {
        assert!(is_failed_startup(Some(false)));
    }

    #[test]
    fn piped_stdin_is_read() {
        assert_eq!(no_path_action(true, false), NoPathAction::ReadStdin);
        assert_eq!(no_path_action(true, true), NoPathAction::ReadStdin);
    }

    #[test]
    fn bare_invocation_on_a_terminal_launches_the_app() {
        assert_eq!(no_path_action(false, false), NoPathAction::LaunchApp);
    }

    #[test]
    fn explicit_dash_on_a_terminal_is_an_error() {
        assert_eq!(no_path_action(false, true), NoPathAction::StdinIsTerminal);
    }

    #[test]
    fn blank_payloads_are_empty() {
        assert!(is_empty_payload(""));
        assert!(is_empty_payload("\n"));
        assert!(is_empty_payload("  \t\r\n "));
    }

    #[test]
    fn payloads_with_content_are_not_empty() {
        assert!(!is_empty_payload("x"));
        assert!(!is_empty_payload("\n# heading\n"));
        assert!(!is_empty_payload("0"));
    }
}
