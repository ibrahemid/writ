//! Argument resolution, note verbs and launch plumbing for the `writ` command.
//!
//! The crate links `writ-core` and `writ-storage` and no Tauri, so it builds
//! and runs without the editor while answering from the same policy and the
//! same note index the app uses (`docs/ARCHITECTURE.md`, ADR-017). The verbs in
//! [`verbs`] read that index; the launch path below shells out to the app.
//!
//! The notes folder is resolved by `writ_core::notes::resolve_notes_root_from`,
//! the function the app resolves it with, and piped text becomes a note
//! through `writ_storage::guarded::create_note_guarded`, the mint every note
//! write goes through: one sanitiser, one dedupe, and the data folder's write
//! lock shared with the app.

/// The command line, read by clap.
pub mod args;
/// The MCP server, reached as `writ mcp`.
pub mod mcp;
/// The note verbs: `links`, `backlinks`, `properties`, `tags`, `new`, `rename`
/// and `trash`.
pub mod verbs;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use writ_core::config::{FileExtension, FilesConfig};
use writ_core::notes::{
    minted_stem, resolve_notes_root_from, sanitize_title, NotesRootError, NotesRootSources,
    WriteOrigin,
};
use writ_storage::errors::{StorageError, StorageResult};
use writ_storage::guarded::{create_note_guarded, CreateNote, TakenName};
use writ_storage::write_lock::WriteLock;

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
    /// `-` was requested but nothing is piped to stdin.
    NothingPiped,
}

/// What stdin is, as far as reading a payload from it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdinKind {
    /// A terminal: nothing is piped.
    Terminal,
    /// A pipe or a regular file: a payload to read.
    Payload,
    /// Anything else: `/dev/null`, a socket, a device. A launcher, an agent's
    /// shell or `spawn()` hands the command one of these, and reading it
    /// either hangs or finds nothing.
    Other,
}

/// Classifies stdin from what the terminal check and `fstat` say about it.
pub fn classify_stdin(is_terminal: bool, is_fifo: bool, is_file: bool) -> StdinKind {
    if is_terminal {
        StdinKind::Terminal
    } else if is_fifo || is_file {
        StdinKind::Payload
    } else {
        StdinKind::Other
    }
}

/// What this process's stdin is.
#[cfg(unix)]
pub fn stdin_kind() -> StdinKind {
    use std::io::IsTerminal;
    use std::os::fd::AsFd;
    use std::os::unix::fs::FileTypeExt;

    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return StdinKind::Terminal;
    }
    let Ok(descriptor) = stdin.as_fd().try_clone_to_owned() else {
        return StdinKind::Other;
    };
    match std::fs::File::from(descriptor).metadata() {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            classify_stdin(false, file_type.is_fifo(), file_type.is_file())
        }
        Err(_) => StdinKind::Other,
    }
}

/// What this process's stdin is.
#[cfg(windows)]
pub fn stdin_kind() -> StdinKind {
    use std::io::IsTerminal;
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileType(handle: *mut std::ffi::c_void) -> u32;
    }
    const FILE_TYPE_DISK: u32 = 0x0001;
    const FILE_TYPE_PIPE: u32 = 0x0003;

    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return StdinKind::Terminal;
    }
    // SAFETY: the handle is this process's own stdin, open for the call.
    let file_type = unsafe { GetFileType(stdin.as_raw_handle()) };
    classify_stdin(
        false,
        file_type == FILE_TYPE_PIPE,
        file_type == FILE_TYPE_DISK,
    )
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

/// The text piped to `writ`, read as the app reads a file
/// (`writ_core::text_encoding::decode_file`, ADR-049), or `None` for bytes
/// the app would show as binary.
///
/// The note it becomes is written in UTF-8, as every note Writ creates is: a
/// pipe has no file whose encoding there is to keep.
pub fn piped_text(bytes: &[u8]) -> Option<String> {
    writ_core::text_encoding::decode_file(bytes).map(|decoded| decoded.text.into_owned())
}

/// Decide what to do when no paths were given.
///
/// A pipe or a file on stdin is read as the payload. Anything else holds
/// nothing to read: an explicit `-` is then an error, while a bare `writ`
/// opens the app.
pub fn no_path_action(stdin: StdinKind, dash_given: bool) -> NoPathAction {
    match stdin {
        StdinKind::Payload => NoPathAction::ReadStdin,
        StdinKind::Terminal | StdinKind::Other if dash_given => NoPathAction::NothingPiped,
        StdinKind::Terminal | StdinKind::Other => NoPathAction::LaunchApp,
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

/// Folder name of the default notes folder, under the user's home folder.
/// Mirrors `writ_core::notes::DEFAULT_NOTES_FOLDER`.
pub const NOTES_FOLDER: &str = "Writ";

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

/// Reads `[files]` out of `<writ_dir>/config.toml`.
///
/// Tolerant the same way [`read_notes_root_from_config`] is: a config that is
/// absent, unreadable or hand-edited into something that will not parse yields
/// the default format rather than ending the run
/// ([`FilesConfig::from_config_text`]). A file the `writ` command writes is a
/// file Writ mints and carries the format the person chose (ADR-041 §2).
pub fn read_files_config(writ_dir: &Path) -> FilesConfig {
    let text = std::fs::read_to_string(writ_dir.join("config.toml")).unwrap_or_default();
    FilesConfig::from_config_text(&text)
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

/// The notes folder the app resolves, which is the folder every key in its
/// version store is relative to.
///
/// [`resolve_notes_dir`] without `WRIT_NOTES_DIR`: that variable is this
/// process's own setting, and a `writ mcp` started on another folder still
/// writes its versions into the app's store, keyed against the app's folder.
/// `None` when no source resolves. Read from `config.toml` on every call, so
/// a folder the app moved to while this process runs is the one answered.
pub fn resolve_app_notes_dir(
    writ_dir: &Path,
    data_dir: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let configured = read_notes_root_from_config(writ_dir);
    resolve_notes_dir(None, configured.as_deref(), data_dir, home).ok()
}

/// How many times a piped write mints again after another writer took the
/// name it picked between the dedupe and the write.
pub const PIPED_WRITE_ATTEMPTS: usize = 8;

/// The stem a piped payload is saved under.
///
/// A `--title` keeps every character a file name may hold
/// ([`writ_core::notes::sanitize_title`]), so `ملاحظات` and `Réunion` survive.
/// With no title, or one that sanitises to nothing, the payload gets a minted
/// name, `writ-<yymmdd>-<hhmm>`, as the window's New File does (ADR-041 §3).
/// The dated name belongs to Today's File: a pipe that took it would make
/// File, Today's File open a test log as the day's note.
pub fn piped_note_stem(title: Option<&str>, now: DateTime<Utc>) -> String {
    title
        .and_then(sanitize_title)
        .unwrap_or_else(|| minted_stem(now))
}

/// Writes piped text as a new note in `notes_dir` and returns its path.
///
/// The note is minted by [`create_note_guarded`] under the write lock in
/// `writ_dir`, the one the app and every other `writ` process share, so the
/// name is deduped against the folder and never replaces a file. Two pipes
/// started together can pick the same free name; the one that loses finds it
/// taken under the lock and mints again, so both payloads are kept.
///
/// # Errors
///
/// [`create_note_guarded`]'s, and [`StorageError::NoteNameTaken`] when every
/// one of [`PIPED_WRITE_ATTEMPTS`] lost its name to another writer.
pub fn write_piped_note(
    notes_dir: &Path,
    writ_dir: &Path,
    stem: &str,
    extension: FileExtension,
    content: &str,
) -> StorageResult<PathBuf> {
    let lock = WriteLock::in_data_folder(writ_dir);
    let mut attempt = 1;
    loop {
        let written = create_note_guarded(
            CreateNote {
                notes_root: notes_dir,
                stem,
                extension: extension.as_str(),
                content,
                origin: WriteOrigin::Cli,
                on_taken_name: TakenName::Dedupe,
                history: None,
                lock: Some(&lock),
            },
            None,
        );
        match written {
            Err(StorageError::NoteNameTaken { .. }) if attempt < PIPED_WRITE_ATTEMPTS => {
                attempt += 1;
            }
            other => return other,
        }
    }
}

/// What `writ` hands `ShellExecuteW` to open a path with its default app.
///
/// The path is the file argument, verbatim and NUL-terminated, and nothing
/// else is passed: no shell reads it, so `&`, `|` and `^` in a file name are
/// characters of the name.
#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOpen {
    /// The verb, `open`.
    pub operation: Vec<u16>,
    /// The path.
    pub file: Vec<u16>,
}

/// The [`ShellOpen`] request for `path`.
#[cfg(windows)]
pub fn shell_open_request(path: &Path) -> ShellOpen {
    use std::os::windows::ffi::OsStrExt;

    let wide = |value: &std::ffi::OsStr| -> Vec<u16> { value.encode_wide().chain([0]).collect() };
    ShellOpen {
        operation: wide(std::ffi::OsStr::new("open")),
        file: wide(path.as_os_str()),
    }
}

/// Opens `path` with its default app through `ShellExecuteW`.
///
/// # Errors
///
/// The value `ShellExecuteW` returned, 32 or less, when it opened nothing.
#[cfg(windows)]
pub fn shell_open(path: &Path) -> Result<(), isize> {
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> *mut std::ffi::c_void;
    }
    const SW_SHOWNORMAL: i32 = 1;

    let request = shell_open_request(path);
    // SAFETY: both strings are NUL-terminated and outlive the call; the null
    // pointers are the documented "none" for the window, the parameters and
    // the working folder.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            request.operation.as_ptr(),
            request.file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(result)
    }
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
    fn a_configured_notes_root_is_where_a_pipe_lands() {
        let dir = TempDir::new().unwrap();
        let configured = dir.path().join("Elsewhere");
        std::fs::write(
            dir.path().join("config.toml"),
            format!("[notes]\nroot = '{}'\n", configured.display()),
        )
        .unwrap();

        let root = read_notes_root_from_config(dir.path()).expect("configured root");
        let notes = resolve_notes_dir(None, Some(&root), None, Some(dir.path()))
            .expect("a configured absolute root resolves");
        assert_eq!(notes, configured);
        let path = write_piped_note(&notes, dir.path(), "my notes", FileExtension::Txt, "x\n")
            .expect("written");
        assert_eq!(path, configured.join("my notes.txt"));
    }

    #[test]
    fn the_default_notes_root_is_home_writ() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read_notes_root_from_config(dir.path()), None);

        let notes =
            resolve_notes_dir(None, None, None, Some(dir.path())).expect("the default resolves");
        assert_eq!(notes, dir.path().join("Writ"));
    }

    #[test]
    fn a_piped_note_dedupes_against_the_folder() {
        let dir = TempDir::new().unwrap();
        touch(&dir.path().join("my notes.txt"));
        touch(&dir.path().join("my notes-2.txt"));

        let path = write_piped_note(
            dir.path(),
            dir.path(),
            "my notes",
            FileExtension::Txt,
            "payload\n",
        )
        .expect("written");
        assert_eq!(path, dir.path().join("my notes-3.txt"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload\n");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("my notes.txt")).unwrap(),
            "",
            "an existing file was replaced"
        );
    }

    #[test]
    fn an_untitled_pipe_never_takes_todays_file() {
        use chrono::TimeZone;
        let morning = chrono::Local
            .with_ymd_and_hms(2026, 10, 3, 9, 41, 0)
            .unwrap()
            .with_timezone(&Utc);
        let stem = piped_note_stem(None, morning);
        assert_eq!(stem, "writ-261003-0941");
        let dir = TempDir::new().unwrap();
        for extension in [FileExtension::Txt, FileExtension::Md] {
            let path = write_piped_note(dir.path(), dir.path(), &stem, extension, "log\n")
                .expect("written");
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert_ne!(
                name,
                writ_core::startup::dated_note_name(morning, extension),
                "the pipe took Today's File"
            );
        }
        assert_eq!(piped_note_stem(Some("  ..  "), morning), "writ-261003-0941");
    }

    #[test]
    fn a_title_keeps_characters_beyond_ascii() {
        let now = Utc::now();
        assert_eq!(piped_note_stem(Some("ملاحظات"), now), "ملاحظات");
        assert_eq!(piped_note_stem(Some("Réunion"), now), "Réunion");
        assert_eq!(piped_note_stem(Some("会议 notes"), now), "会议 notes");
        assert_eq!(piped_note_stem(Some("a/b:c"), now), "a b c");

        let dir = TempDir::new().unwrap();
        let path = write_piped_note(
            dir.path(),
            dir.path(),
            &piped_note_stem(Some("ملاحظات"), now),
            FileExtension::Md,
            "نص\n",
        )
        .expect("written");
        assert_eq!(path, dir.path().join("ملاحظات.md"));
    }

    #[test]
    fn a_title_that_would_escape_the_folder_cannot() {
        let dir = TempDir::new().unwrap();
        let notes = dir.path().join("notes");
        let stem = piped_note_stem(Some("../escape"), Utc::now());
        let path = write_piped_note(&notes, dir.path(), &stem, FileExtension::Txt, "x\n")
            .expect("written");
        assert!(
            path.starts_with(&notes),
            "the note escaped the notes folder: {path:?}"
        );
    }

    #[test]
    fn two_pipes_at_once_keep_both_payloads() {
        let dir = TempDir::new().unwrap();
        let notes = dir.path().join("notes");
        let writ = dir.path().join("data");
        std::fs::create_dir_all(&writ).unwrap();
        for round in 0..20 {
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let handles: Vec<_> = (0..2)
                .map(|which| {
                    let (notes, writ, barrier) = (notes.clone(), writ.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        write_piped_note(
                            &notes,
                            &writ,
                            &format!("race {round}"),
                            FileExtension::Txt,
                            &format!("payload {which}\n"),
                        )
                        .expect("written")
                    })
                })
                .collect();
            let paths: Vec<PathBuf> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            assert_ne!(paths[0], paths[1], "round {round}: one name for two pipes");
            let mut texts: Vec<String> = paths
                .iter()
                .map(|path| std::fs::read_to_string(path).unwrap())
                .collect();
            texts.sort();
            assert_eq!(texts, ["payload 0\n", "payload 1\n"], "round {round}");
        }
    }

    #[test]
    fn read_files_config_reads_the_table_and_tolerates_everything_else() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            read_files_config(dir.path()).default_extension,
            FileExtension::Txt,
            "a config that has never been written names the default"
        );

        std::fs::write(
            dir.path().join("config.toml"),
            "[notes]\nroot = '~/Writ'\n\n[files]\ndefault_extension = \"md\"\n",
        )
        .unwrap();
        assert_eq!(
            read_files_config(dir.path()).default_extension,
            FileExtension::Md
        );

        std::fs::write(dir.path().join("config.toml"), "[files\nnot toml at all").unwrap();
        assert_eq!(
            read_files_config(dir.path()).default_extension,
            FileExtension::Txt,
            "a config nothing can parse never stops a pipe"
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
    fn the_apps_notes_folder_is_the_configured_one() {
        let dir = TempDir::new().unwrap();
        let configured = dir.path().join("Configured");
        std::fs::write(
            dir.path().join("config.toml"),
            format!("[notes]\nroot = '{}'\n", configured.display()),
        )
        .unwrap();

        assert_eq!(
            resolve_app_notes_dir(dir.path(), None, Some(dir.path())),
            Some(configured)
        );
    }

    #[test]
    fn the_apps_notes_folder_falls_back_the_way_the_app_does() {
        let dir = TempDir::new().unwrap();
        let data = dir.path().join("instance");

        assert_eq!(
            resolve_app_notes_dir(dir.path(), None, Some(dir.path())),
            Some(dir.path().join("Writ"))
        );
        assert_eq!(
            resolve_app_notes_dir(dir.path(), Some(&data), Some(dir.path())),
            Some(data.join("Writ"))
        );
    }

    #[test]
    fn the_apps_notes_folder_follows_a_config_rewritten_after_the_first_answer() {
        let dir = TempDir::new().unwrap();
        let first = dir.path().join("First");
        let moved = dir.path().join("Moved");
        let config = dir.path().join("config.toml");
        std::fs::write(&config, format!("[notes]\nroot = '{}'\n", first.display())).unwrap();
        assert_eq!(
            resolve_app_notes_dir(dir.path(), None, Some(dir.path())),
            Some(first)
        );

        std::fs::write(&config, format!("[notes]\nroot = '{}'\n", moved.display())).unwrap();

        assert_eq!(
            resolve_app_notes_dir(dir.path(), None, Some(dir.path())),
            Some(moved)
        );
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
    fn a_pipe_or_a_file_on_stdin_is_read() {
        assert_eq!(classify_stdin(false, true, false), StdinKind::Payload);
        assert_eq!(classify_stdin(false, false, true), StdinKind::Payload);
        assert_eq!(
            no_path_action(StdinKind::Payload, false),
            NoPathAction::ReadStdin
        );
        assert_eq!(
            no_path_action(StdinKind::Payload, true),
            NoPathAction::ReadStdin
        );
    }

    #[test]
    fn stdin_that_is_neither_a_pipe_nor_a_file_launches_the_app() {
        // `/dev/null`, a socket or a device: what a launcher or `spawn()`
        // hands the command.
        assert_eq!(classify_stdin(false, false, false), StdinKind::Other);
        assert_eq!(
            no_path_action(StdinKind::Other, false),
            NoPathAction::LaunchApp
        );
        assert_eq!(
            no_path_action(StdinKind::Other, true),
            NoPathAction::NothingPiped
        );
    }

    #[cfg(unix)]
    #[test]
    fn dev_null_is_not_a_payload() {
        use std::os::unix::fs::FileTypeExt;
        let file_type = std::fs::metadata("/dev/null").unwrap().file_type();
        assert_eq!(
            classify_stdin(false, file_type.is_fifo(), file_type.is_file()),
            StdinKind::Other
        );
    }

    #[test]
    fn bare_invocation_on_a_terminal_launches_the_app() {
        assert_eq!(classify_stdin(true, false, false), StdinKind::Terminal);
        assert_eq!(
            no_path_action(StdinKind::Terminal, false),
            NoPathAction::LaunchApp
        );
    }

    #[test]
    fn explicit_dash_on_a_terminal_is_an_error() {
        assert_eq!(
            no_path_action(StdinKind::Terminal, true),
            NoPathAction::NothingPiped
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_opens_a_path_with_shell_characters_without_a_shell() {
        use std::os::windows::ffi::OsStrExt;

        let path = Path::new(r"C:\Users\me\Downloads\a&calc");
        let request = shell_open_request(path);
        let expected: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        assert_eq!(request.file, expected, "the path is passed verbatim");
        let operation = String::from_utf16_lossy(&request.operation);
        assert_eq!(operation, "open\0");
        let file = String::from_utf16_lossy(&request.file);
        assert!(!file.to_lowercase().contains("cmd"), "{file}");
        assert!(
            !file.contains('"'),
            "the path was quoted for a shell: {file}"
        );
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

    #[test]
    fn piped_text_in_another_encoding_is_read_as_the_app_reads_it() {
        assert_eq!(piped_text("café\n".as_bytes()).as_deref(), Some("café\n"));
        assert_eq!(
            piped_text(b"caf\xe9 cr\xe8me\n").as_deref(),
            Some("café crème\n")
        );
        // UTF-16LE with a byte order mark, as PowerShell writes it.
        let utf16: Vec<u8> = b"\xff\xfe"
            .iter()
            .copied()
            .chain("dir\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(piped_text(&utf16).as_deref(), Some("dir\r\n"));
    }

    #[test]
    fn piped_binary_is_not_text() {
        assert_eq!(
            piped_text(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00"),
            None
        );
    }
}
