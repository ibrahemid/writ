use std::ffi::OsString;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process;

use writ_cli::args::{self, Invocation};
use writ_cli::handoff::{self, Delivered};
use writ_cli::mcp;
use writ_cli::verbs::{self, OpenPlan};
use writ_cli::{
    is_empty_payload, no_path_action, piped_note_stem, piped_text, read_files_config,
    read_notes_root_from_config, resolve_app_notes_dir, resolve_notes_dir, resolve_targets,
    split_line_targets, stdin_kind, write_piped_note, NoPathAction, OpenTarget, StdinKind,
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

fn main() {
    // Keeps the signature in the binary, where the app's install looks for it
    // before it replaces a file named `writ` (ADR-017).
    std::hint::black_box(writ_core::cli_install::CLI_SIGNATURE);

    let invocation = match args::parse_invocation(std::env::args_os()) {
        Ok(invocation) => invocation,
        Err(error) => error.exit(),
    };
    match invocation {
        Invocation::Mcp => run_mcp(),
        Invocation::Verb(verb) => run_verb(verb),
        Invocation::Open {
            paths,
            title,
            wait,
            no_open,
        } => open_paths(paths, title, wait, no_open),
    }
}

/// The file-opening path: open files or a folder, read piped stdin, or launch
/// the app.
fn open_paths(paths: Vec<OsString>, title: Option<String>, wait: bool, no_open: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let stdin = stdin_kind();
    let dash_given = paths.len() == 1 && paths[0] == "-";
    if no_open && !(paths.is_empty() || dash_given) {
        eprintln!("writ: --no-open is for piped text; a file given by path is already saved");
        process::exit(verbs::EXIT_USAGE);
    }

    let (paths, lines) = split_line_targets(&paths, &cwd);
    let target = match resolve_targets(&paths, &cwd, title) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("writ: {e}");
            process::exit(verbs::EXIT_USAGE);
        }
    };

    match target {
        OpenTarget::Files(paths) => {
            open_plan(&OpenPlan { paths, lines, wait });
        }
        OpenTarget::Workspace(dir) => {
            if wait {
                eprintln!(
                    "writ: --wait waits for files, and {} is a folder",
                    dir.display()
                );
                process::exit(verbs::EXIT_USAGE);
            }
            launch_writ(std::slice::from_ref(&dir));
        }
        OpenTarget::Stdin { title } => match no_path_action(stdin, dash_given) {
            NoPathAction::LaunchApp => {
                launch_writ(&[]);
            }
            NoPathAction::NothingPiped => {
                eprintln!("writ: `-` was given but nothing is piped to stdin");
                eprintln!("Usage: writ [PATH...]  or pipe content via stdin");
                process::exit(verbs::EXIT_USAGE);
            }
            NoPathAction::ReadStdin => {
                let mut piped = Vec::new();
                if let Err(e) = io::stdin().read_to_end(&mut piped) {
                    eprintln!("writ: failed to read stdin: {e}");
                    process::exit(verbs::EXIT_FAILED);
                }
                let Some(content) = piped_text(&piped) else {
                    eprintln!("writ: stdin is not text");
                    process::exit(verbs::EXIT_FAILED);
                };

                if is_empty_payload(&content) {
                    if no_open {
                        eprintln!("writ: nothing was piped, so nothing was saved");
                    }
                    return;
                }

                let (writ_dir, notes_dir) = writ_paths();
                let stem = piped_note_stem(title.as_deref(), chrono::Utc::now());
                let extension = read_files_config(&writ_dir).default_extension;
                let dest = match write_piped_note(&notes_dir, &writ_dir, &stem, extension, &content)
                {
                    Ok(dest) => dest,
                    Err(error) => {
                        eprintln!(
                            "writ: cannot write to {}: {}",
                            notes_dir.display(),
                            verbs::plain_storage_reason(&error)
                        );
                        process::exit(verbs::EXIT_FAILED);
                    }
                };

                if no_open {
                    println!("{}", dest.display());
                    return;
                }
                open_plan(&OpenPlan {
                    paths: vec![dest],
                    wait,
                    ..OpenPlan::default()
                });
            }
        },
    }
}

/// Opens `plan` in the app, through the folder the running app takes
/// requests from when it runs ([`handoff::deliver`]), and exits 1 when it was
/// not opened.
fn open_plan(plan: &OpenPlan) {
    match handoff::deliver(&writ_dir(), plan, launch_writ, handoff::Timings::default()) {
        Ok(Delivered::Opened | Delivered::Closed | Delivered::AppEnded) => {}
        Err(error) => {
            eprintln!("writ: {error}");
            process::exit(verbs::EXIT_FAILED);
        }
    }
}

/// The data folder holding `writ.db` and `config.toml`, and the notes folder.
///
/// Both are resolved from the same three sources in the same order the app
/// resolves them from, so the CLI and the app never disagree about where a note
/// goes or which index describes it.
/// A notes folder that cannot be resolved stops the command here rather than
/// letting it write somewhere the app would never look. The refusal names the
/// path, which is the thing the user can go and fix.
fn writ_paths() -> (PathBuf, PathBuf) {
    let data_dir_override = data_dir_override();
    let home = dirs::home_dir();
    let writ_dir = writ_dir();
    let configured = read_notes_root_from_config(&writ_dir);
    let env_override = std::env::var("WRIT_NOTES_DIR").ok();

    match resolve_notes_dir(
        env_override.as_deref(),
        configured.as_deref(),
        data_dir_override.as_deref(),
        home.as_deref(),
    ) {
        Ok(notes) => (writ_dir, notes),
        Err(error) => {
            eprintln!("writ: {error}");
            process::exit(1);
        }
    }
}

/// The data folder holding `writ.db`, `config.toml` and the requests the
/// running app takes: `WRIT_DATA_DIR`, or `.writ` in the home folder.
fn writ_dir() -> PathBuf {
    data_dir_override().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".writ")
    })
}

/// The data folder `WRIT_DATA_DIR` names, when it names one.
fn data_dir_override() -> Option<PathBuf> {
    std::env::var("WRIT_DATA_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Serves the MCP server on stdio and exits. Never returns: a client owns this
/// process for as long as it holds the pipe.
///
/// The index is opened read-only and is never created here, so a folder the app
/// has not indexed yet serves its notes and refuses the index-derived tools
/// until the app builds one, rather than writing a database a second process
/// would then be sharing.
fn run_mcp() -> ! {
    let (writ_dir, notes_dir) = writ_paths();
    // The gate reads `[mcp]` itself, on every call: this process is long-lived
    // and an approval granted in the app has to reach it without a restart
    // (ADR-031 rule 3.5). The folder is read again whenever the settings
    // change, for the same reason, unless `WRIT_NOTES_DIR` pins it.
    let gate = Box::new(writ_mcp::consent::ConfigGate::new(&writ_dir));
    let app_folder = Box::new(mcp::ConfiguredAppFolder {
        writ_dir: writ_dir.clone(),
        data_dir: data_dir_override(),
        home: dirs::home_dir(),
    });
    let db_path = writ_dir.join("writ.db");
    let opened = if mcp::notes_dir_is_pinned(std::env::var("WRIT_NOTES_DIR").ok().as_deref()) {
        writ_mcp::tools::ToolHost::open(&notes_dir, &db_path, &writ_dir, app_folder, gate)
    } else {
        writ_mcp::tools::ToolHost::open_following_app(&db_path, &writ_dir, app_folder, gate)
    };
    let host = match opened {
        Ok(host) => host
            .with_opener(Box::new(mcp::LinkHandoff {
                writ_dir: writ_dir.clone(),
                launch: start_writ_quietly,
            }))
            .with_context_source(Box::new(mcp::ContextHandoff {
                writ_dir: writ_dir.clone(),
            }))
            .with_review_source(Box::new(mcp::ReviewHandoff {
                writ_dir: writ_dir.clone(),
            })),
        Err(error) => {
            eprintln!("writ: {error}");
            process::exit(verbs::EXIT_FAILED);
        }
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("writ: the server could not start: {error}");
            process::exit(verbs::EXIT_FAILED);
        }
    };
    match runtime.block_on(writ_mcp::server::serve_stdio(host)) {
        Ok(()) => process::exit(verbs::EXIT_OK),
        Err(error) => {
            eprintln!("writ: {error}");
            process::exit(verbs::EXIT_FAILED);
        }
    }
}

/// Runs a note verb and exits. Never returns: the verbs are a surface of their
/// own and none of them falls through to the file-opening path.
fn run_verb(verb: verbs::Verb) -> ! {
    let (writ_dir, notes_dir) = writ_paths();
    let stdin = if verb.reads_stdin() {
        read_piped_stdin()
    } else {
        None
    };
    let context = verbs::Context {
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        notes_dir,
        db_path: writ_dir.join("writ.db"),
        now: chrono::Utc::now(),
        default_extension: read_files_config(&writ_dir).default_extension,
        app_notes_dir: resolve_app_notes_dir(
            &writ_dir,
            data_dir_override().as_deref(),
            dirs::home_dir().as_deref(),
        ),
        writ_dir: writ_dir.clone(),
        stdin,
    };

    let outcome = verbs::run(verb, &context);
    if outcome.code == verbs::EXIT_OK {
        if let Some(plan) = &outcome.open {
            open_plan(plan);
        }
    }
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    process::exit(outcome.code);
}

/// The text piped to stdin, or `None` when stdin is a terminal or nothing a
/// pipe could fill.
///
/// Bytes that are not text end the command with exit 1.
fn read_piped_stdin() -> Option<String> {
    if stdin_kind() != StdinKind::Payload {
        return None;
    }
    let mut piped = Vec::new();
    if let Err(e) = io::stdin().read_to_end(&mut piped) {
        eprintln!("writ: failed to read stdin: {e}");
        process::exit(verbs::EXIT_FAILED);
    }
    match piped_text(&piped) {
        Some(text) => Some(text),
        None => {
            eprintln!("writ: stdin is not text");
            process::exit(verbs::EXIT_FAILED);
        }
    }
}

/// Starts Writ with nothing to open, for `writ mcp`, answering whether it did.
///
/// Every stream of the started process is closed: this process's stdout is
/// the protocol, and a byte from anything else on it ends the client's
/// session.
#[cfg(target_os = "macos")]
fn start_writ_quietly() -> bool {
    process::Command::new("open")
        .arg("-b")
        .arg(MACOS_BUNDLE_ID)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Starts Writ with nothing to open, for `writ mcp`, answering whether it did.
///
/// Every stream of the started process is closed: this process's stdout is
/// the protocol, and a byte from anything else on it ends the client's
/// session.
#[cfg(not(target_os = "macos"))]
fn start_writ_quietly() -> bool {
    let current_exe = std::env::current_exe().ok();
    let env_override = std::env::var_os(GUI_BIN_ENV).map(PathBuf::from);
    let GuiLaunch::Binary(bin) =
        resolve_gui_binary(env_override.as_deref(), current_exe.as_deref())
    else {
        return false;
    };
    let mut command = process::Command::new(bin);
    command
        .env_remove(GUI_BIN_ENV)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().is_ok()
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
    if let Err(code) = writ_cli::shell_open(path) {
        eprintln!("writ: failed to open {} (error {code})", path.display());
        process::exit(verbs::EXIT_FAILED);
    }
}
