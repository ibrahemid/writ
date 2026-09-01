use std::ffi::OsString;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process;

use clap::Parser;
use writ_cli::verbs;
use writ_cli::{
    is_empty_payload, no_path_action, piped_note_path, read_notes_root_from_config,
    resolve_notes_dir, resolve_targets, NoPathAction, OpenTarget,
};

#[cfg(not(target_os = "macos"))]
use std::path::Path;
#[cfg(not(target_os = "macos"))]
use std::thread;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;
#[cfg(not(target_os = "macos"))]
use writ_cli::{is_failed_startup, resolve_gui_binary, GuiLaunch, GUI_BIN_ENV};

#[cfg(target_os = "macos")]
const MACOS_BUNDLE_ID: &str = "com.writ.editor";

const ENV_HELP: &str = "Environment:\n  WRIT_GUI_BIN  Path to the Writ application binary. Read on Linux and Windows,\n                where it takes precedence over the binary installed next to\n                this one.\n  WRIT_NOTES_DIR  The notes folder to read and write, overriding the setting.\n  WRIT_DATA_DIR   The folder holding writ.db and config.toml.";

#[derive(Parser)]
#[command(
    name = "writ",
    about = "Open files, folders, or piped input in Writ",
    after_help = ENV_HELP,
    version
)]
struct Cli {
    /// Files or directories to open. Pass `-` for piped stdin, or omit to open Writ.
    #[arg(value_name = "PATH")]
    paths: Vec<OsString>,

    /// Title for the piped stdin buffer tab.
    #[arg(long, value_name = "TITLE")]
    title: Option<String>,
}

fn main() {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Some(parsed) = verbs::parse(&argv) {
        run_verb(parsed);
    }

    let cli = Cli::parse();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let is_pipe = !io::stdin().is_terminal();
    let dash_given = cli.paths.len() == 1 && cli.paths[0] == "-";
    let effective_paths: Vec<OsString> = if is_pipe && cli.paths.is_empty() {
        vec![]
    } else {
        cli.paths
    };

    let target = match resolve_targets(&effective_paths, &cwd, cli.title.clone()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("writ: {e}");
            process::exit(1);
        }
    };

    match target {
        OpenTarget::Files(paths) => {
            launch_writ(&paths);
        }
        OpenTarget::Workspace(dir) => {
            launch_writ(std::slice::from_ref(&dir));
        }
        OpenTarget::Stdin { title } => match no_path_action(is_pipe, dash_given) {
            NoPathAction::LaunchApp => {
                launch_writ(&[]);
            }
            NoPathAction::StdinIsTerminal => {
                eprintln!("writ: `-` was given but stdin is a terminal");
                eprintln!("Usage: writ [PATH...]  or pipe content via stdin");
                process::exit(1);
            }
            NoPathAction::ReadStdin => {
                let mut content = String::new();
                if let Err(e) = io::stdin().read_to_string(&mut content) {
                    eprintln!("writ: failed to read stdin: {e}");
                    process::exit(1);
                }

                if is_empty_payload(&content) {
                    return;
                }

                let notes_dir = notes_dir();
                if let Err(e) = std::fs::create_dir_all(&notes_dir) {
                    eprintln!("writ: cannot create {}: {e}", notes_dir.display());
                    process::exit(1);
                }

                let dest = piped_note_path(&notes_dir, title.as_deref(), chrono::Local::now());

                if let Err(e) = std::fs::write(&dest, &content) {
                    eprintln!("writ: cannot write to {}: {e}", dest.display());
                    process::exit(1);
                }

                launch_writ(std::slice::from_ref(&dest));
            }
        },
    }
}

/// The notes folder piped input lands in.
///
/// Piped text becomes a note in the notes folder like any other (ADR-028 §1),
/// so it is a file the user can see in Finder and Writ can open without any
/// further permission. `~/.writ/piped/` is gone; what it already holds is
/// moved into the notes folder by the app's one-time pass.
///
/// The folder is resolved the way the app resolves it, from the same three
/// sources in the same order, so the CLI and the app never disagree about
/// where a note goes.
fn notes_dir() -> PathBuf {
    writ_paths().1
}

/// The data folder holding `writ.db` and `config.toml`, and the notes folder.
///
/// Both are resolved from the same three sources in the same order the app
/// resolves them from, so the CLI and the app never disagree about where a note
/// goes or which index describes it.
fn writ_paths() -> (PathBuf, PathBuf) {
    let data_dir_override = std::env::var("WRIT_DATA_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty());
    let home = dirs::home_dir();
    let writ_dir = data_dir_override.clone().unwrap_or_else(|| {
        home.clone()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".writ")
    });
    let configured = read_notes_root_from_config(&writ_dir);
    let env_override = std::env::var("WRIT_NOTES_DIR").ok();

    let notes = resolve_notes_dir(
        env_override.as_deref(),
        configured.as_deref(),
        data_dir_override.as_deref(),
        home.as_deref(),
    );
    (writ_dir, notes)
}

/// Runs a note verb and exits. Never returns: the verbs are a surface of their
/// own and none of them falls through to the file-opening path.
fn run_verb(parsed: Result<verbs::Verb, verbs::UsageError>) -> ! {
    let verb = match parsed {
        Ok(verb) => verb,
        Err(error) => {
            eprintln!("writ: {error}");
            eprintln!("{}", verbs::usage());
            process::exit(verbs::EXIT_USAGE);
        }
    };

    let (writ_dir, notes_dir) = writ_paths();
    let context = verbs::Context {
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        notes_dir,
        db_path: writ_dir.join("writ.db"),
        now: chrono::Utc::now(),
    };

    let outcome = verbs::run(verb, &context);
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    process::exit(outcome.code);
}

/// Open `paths` in Writ, or launch Writ with no document when `paths` is empty.
#[cfg(target_os = "macos")]
fn launch_writ(paths: &[PathBuf]) {
    let mut command = process::Command::new("open");
    command.arg("-b").arg(MACOS_BUNDLE_ID).args(paths);

    match command.status() {
        Ok(status) if status.success() => {}
        // `open` has already written its own diagnostic to stderr.
        Ok(_) => process::exit(1),
        Err(e) => {
            eprintln!("writ: could not open the Writ app: {e}");
            process::exit(1);
        }
    }
}

/// Open `paths` in Writ, or launch Writ with no document when `paths` is empty.
///
/// The application registers its file associations at `Alternate` rank, so the
/// desktop default handler for a path is generally not Writ. The resolved
/// binary is launched directly; a running instance picks the paths up through
/// tauri-plugin-single-instance.
#[cfg(not(target_os = "macos"))]
fn launch_writ(paths: &[PathBuf]) {
    let current_exe = std::env::current_exe().ok();
    let env_override = std::env::var_os(GUI_BIN_ENV).map(PathBuf::from);

    match resolve_gui_binary(env_override.as_deref(), current_exe.as_deref()) {
        GuiLaunch::Binary(bin) => spawn_gui(&bin, paths),
        GuiLaunch::OsDefault => {
            if paths.is_empty() {
                eprintln!("writ: could not locate the Writ app");
                eprintln!(
                    "Set {GUI_BIN_ENV} to the Writ app binary so the writ command can launch it."
                );
                process::exit(1);
            }
            eprintln!("writ: could not locate the Writ app, opening with the desktop default");
            for path in paths {
                open_with_os_default(path);
            }
        }
    }
}

/// How long to watch a spawned app process before treating it as started.
#[cfg(not(target_os = "macos"))]
const STARTUP_WINDOW: Duration = Duration::from_millis(300);

#[cfg(not(target_os = "macos"))]
fn spawn_gui(bin: &Path, paths: &[PathBuf]) {
    let mut command = process::Command::new(bin);
    command
        .args(paths)
        // A launcher named by WRIT_GUI_BIN may run this CLI in turn, which would
        // resolve the same launcher again if the variable were inherited.
        .env_remove(GUI_BIN_ENV)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("writ: could not launch {}: {e}", bin.display());
            process::exit(1);
        }
    };

    thread::sleep(STARTUP_WINDOW);
    if let Ok(Some(status)) = child.try_wait() {
        if is_failed_startup(Some(status.success())) {
            eprintln!(
                "writ: {} exited without opening anything ({status})",
                bin.display()
            );
            process::exit(1);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_os_default(path: &Path) {
    if let Err(e) = process::Command::new("xdg-open").arg(path).status() {
        eprintln!("writ: failed to open {}: {e}", path.display());
        process::exit(1);
    }
}

#[cfg(windows)]
fn open_with_os_default(path: &Path) {
    let result = process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .status();

    if let Err(e) = result {
        eprintln!("writ: failed to open {}: {e}", path.display());
        process::exit(1);
    }
}
