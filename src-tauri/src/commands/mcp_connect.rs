//! Connecting an MCP client from its card in Settings (ADR-031 rule 2.4,
//! decision AM-D7).
//!
//! The webview names a client and nothing else. The arguments, the link and
//! the bundle are built here, from `writ_core::mcp_clients` and the command
//! every client is handed (`activity::resolved_command`), so no script in the
//! window can have Writ run or open anything of its choosing. Writ runs the
//! client's own `mcp add`, opens the install link the client handles, or hands
//! the system a bundle the client installs: it never writes another program's
//! configuration.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use writ_core::activity::OffCall;
use writ_core::mcp_clients::{
    bundle_manifest, other_client_json, probe_requests, read_probe_line, Connection, McpClient,
    ProbeLine, BUNDLE_LAUNCHER, SERVE_VERB, USUAL_PROGRAM_FOLDERS,
};

use crate::events::emitter::WritFrontendEvent;
use crate::state::AppState;

/// How long a client's `mcp add` is given.
pub const ADD_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the connection test waits for the server's tool list.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The most of a client's own output a failure carries.
const MAX_SAID: usize = 300;

/// Why connecting a client, or testing the server, did not finish.
///
/// Each message is a sentence for the card that shows it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectError {
    /// The command a client would be handed moves or is not installed yet,
    /// so handing it out now would break the client later (U41).
    #[error("Writ's command is not ready to give a program yet.")]
    CommandNotReady,
    /// The card asked for a way this client is not connected.
    #[error("This program is connected another way.")]
    WrongConnection,
    /// The client's command was found in no folder Writ looked in.
    #[error("The {program} command was not found on this computer.")]
    ProgramNotFound {
        /// The command looked for.
        program: String,
    },
    /// The client's command was found and could not be started.
    #[error("The {program} command could not be started: {reason}.")]
    ProgramNotStarted {
        /// The command.
        program: String,
        /// What the system said.
        reason: String,
    },
    /// The client's command was still running when its time ran out.
    #[error("The {program} command did not finish within {seconds} seconds.")]
    ProgramTimedOut {
        /// The command.
        program: String,
        /// The time it was given.
        seconds: u64,
    },
    /// The client's command ended with a failure, and this is what it said.
    #[error("The {program} command could not add Writ: {said}.")]
    ProgramFailed {
        /// The command.
        program: String,
        /// Its own output, trimmed.
        said: String,
    },
    /// No app took the install link.
    #[error("No app on this computer opened the link. Install the program first.")]
    LinkNotOpened,
    /// The bundle could not be written into the data folder.
    #[error("Writ.mcpb could not be written: {0}.")]
    BundleNotWritten(String),
    /// No app took the bundle.
    #[error("No app on this computer opened Writ.mcpb. Install Claude Desktop first.")]
    BundleNotOpened,
    /// The server could not be started for the test.
    #[error("Writ's server could not be started: {0}.")]
    ServerNotStarted(String),
    /// The server did not list its tools in time.
    #[error("Writ's server did not answer within {0} seconds.")]
    ServerSilent(u64),
    /// The server ended before it listed its tools.
    #[error("Writ's server stopped before it listed its tools: {0}.")]
    ServerEnded(String),
    /// The server answered the test with an error.
    #[error("Writ's server answered with an error: {0}.")]
    ServerFailed(String),
}

/// One client's card: how it connects and what it is handed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ClientCard {
    /// The client.
    pub client: McpClient,
    /// How it connects.
    pub connection: Connection,
    /// The `mcp add` line or the JSON to copy, when the client takes one.
    /// `None` while the command is not ready to hand out.
    pub copy_text: Option<String>,
    /// Whether the client's own command was found, so Add runs it.
    pub runs_command: bool,
}

/// What a client's `mcp add` printed when it succeeded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AddResult {
    /// Its last line of output, trimmed, or empty.
    pub said: String,
}

/// What the connection test found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ServerTest {
    /// How many tools the server listed.
    pub tools: usize,
}

// --- Finding a client's command ---------------------------------------------

/// Where a client's command is looked for, and the `PATH` it is run with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramSearch {
    /// The folders, in the order they are tried, without repeats.
    pub folders: Vec<PathBuf>,
    /// The same folders as a `PATH` value, for the command's own children:
    /// an npm-installed client is a script that finds `node` on the `PATH`.
    pub path: OsString,
}

/// The folders a client's command is looked for in: the login shell's `PATH`,
/// then the app's own, then [`USUAL_PROGRAM_FOLDERS`]. An app opened from the
/// Dock inherits a `PATH` without the folders a terminal adds.
pub fn program_search(
    login_path: Option<&str>,
    inherited: Option<&OsStr>,
    home: Option<&Path>,
) -> ProgramSearch {
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut push = |folder: PathBuf| {
        if folder.is_absolute() && !folders.contains(&folder) {
            folders.push(folder);
        }
    };
    if let Some(login) = login_path {
        std::env::split_paths(OsStr::new(login)).for_each(&mut push);
    }
    if let Some(inherited) = inherited {
        std::env::split_paths(inherited).for_each(&mut push);
    }
    for usual in USUAL_PROGRAM_FOLDERS {
        let usual = Path::new(usual);
        if usual.is_absolute() {
            push(usual.to_path_buf());
        } else if let Some(home) = home {
            push(home.join(usual));
        }
    }
    let path = std::env::join_paths(&folders).unwrap_or_default();
    ProgramSearch { folders, path }
}

/// The first file named `program` in `search`'s folders that can be run.
/// On Windows each `PATHEXT` extension is tried, since npm installs a
/// client as `claude.cmd`.
pub fn find_program(program: &str, search: &ProgramSearch) -> Option<PathBuf> {
    let names = program_file_names(program);
    search
        .folders
        .iter()
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .find(|candidate| is_runnable(candidate))
}

#[cfg(windows)]
fn program_file_names(program: &str) -> Vec<String> {
    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    extensions
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| format!("{program}{}", extension.to_ascii_lowercase()))
        .collect()
}

#[cfg(not(windows))]
fn program_file_names(program: &str) -> Vec<String> {
    vec![program.to_string()]
}

#[cfg(unix)]
fn is_runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_runnable(path: &Path) -> bool {
    path.is_file()
}

/// The `PATH` the person's login shell sets, read by running it once.
///
/// Interactive as well as login, so the startup file most people add their
/// tools in is read too. Its stdin is closed and it is stopped after five
/// seconds, so a startup file that waits for input costs that much and no
/// more.
#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    use writ_core::mcp_clients::{login_shell_path_script, read_login_shell_path};
    const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
    let shell = std::env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| OsString::from("/bin/sh"));
    let mut command = Command::new(shell);
    command.args(["-i", "-l", "-c", &login_shell_path_script()]);
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    let output = run_to_end(command, SHELL_TIMEOUT).ok()?;
    read_login_shell_path(&output.stdout)
}

#[cfg(not(unix))]
fn login_shell_path() -> Option<String> {
    None
}

/// Where this app looks for a client's command now.
fn current_search() -> ProgramSearch {
    program_search(
        login_shell_path().as_deref(),
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
    )
}

// --- Running a command to its end -------------------------------------------

/// What a finished command printed.
#[derive(Debug)]
struct Finished {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
enum RunError {
    NotStarted(String),
    TimedOut,
}

/// Runs `command` with its stdin closed until it exits or `timeout` passes,
/// and stops it then.
fn run_to_end(mut command: Command, timeout: Duration) -> Result<Finished, RunError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| RunError::NotStarted(error.to_string()))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) | Err(_) => {
                stop(&mut child);
                return Err(RunError::TimedOut);
            }
        }
    };
    Ok(Finished {
        success: status.success(),
        stdout: stdout.text(),
        stderr: stderr.text(),
    })
}

/// How long a pipe is read after its process ended: a process it started
/// in the background can hold the pipe open, and its output is not waited on.
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// Reads `pipe` to its end on a thread of its own.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Drained {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    Drained(receiver)
}

/// What a pipe held, once read to its end.
struct Drained(mpsc::Receiver<String>);

impl Drained {
    /// The text, or nothing if the pipe is still open after [`PIPE_GRACE`].
    fn text(self) -> String {
        self.0.recv_timeout(PIPE_GRACE).unwrap_or_default()
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The last line of `text` that says something, held to [`MAX_SAID`]
/// characters, without a closing full stop.
fn last_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("");
    let line: String = line.chars().take(MAX_SAID).collect();
    line.trim_end_matches('.').to_string()
}

// --- The cards ----------------------------------------------------------------

/// Every card, for the command at `server`. `ready` is false while the command
/// moves or is not installed: the cards then carry nothing to copy or run.
pub fn client_cards_inner(server: &Path, ready: bool, search: &ProgramSearch) -> Vec<ClientCard> {
    let server = server.display().to_string();
    McpClient::ALL
        .iter()
        .map(|&client| {
            let copy_text = match client.connection() {
                Connection::Command => client.add_command_line(&server),
                Connection::Json => Some(other_client_json(&server)),
                Connection::Link | Connection::Bundle => None,
            }
            .filter(|_| ready);
            let runs_command = ready
                && client
                    .program()
                    .is_some_and(|program| find_program(program, search).is_some());
            ClientCard {
                client,
                connection: client.connection(),
                copy_text,
                runs_command,
            }
        })
        .collect()
}

/// Runs `client`'s own `mcp add` for the command at `server`, from the home
/// folder, so a client whose default scope is the folder it runs in does not
/// write there.
pub fn add_with_client_inner(
    client: McpClient,
    server: &Path,
    search: &ProgramSearch,
    home: &Path,
    timeout: Duration,
) -> Result<AddResult, ConnectError> {
    let (Some(program), Some(args)) = (
        client.program(),
        client.add_args(&server.display().to_string()),
    ) else {
        return Err(ConnectError::WrongConnection);
    };
    let found = find_program(program, search).ok_or_else(|| ConnectError::ProgramNotFound {
        program: program.to_string(),
    })?;
    let mut command = Command::new(found);
    command
        .args(args)
        .current_dir(home)
        .env("PATH", &search.path);
    match run_to_end(command, timeout) {
        Ok(finished) if finished.success => Ok(AddResult {
            said: last_line(&finished.stdout),
        }),
        Ok(finished) => {
            let said = match last_line(&finished.stderr) {
                said if said.is_empty() => last_line(&finished.stdout),
                said => said,
            };
            Err(ConnectError::ProgramFailed {
                program: program.to_string(),
                said: if said.is_empty() {
                    "it gave no reason".to_string()
                } else {
                    said
                },
            })
        }
        Err(RunError::NotStarted(reason)) => Err(ConnectError::ProgramNotStarted {
            program: program.to_string(),
            reason,
        }),
        Err(RunError::TimedOut) => Err(ConnectError::ProgramTimedOut {
            program: program.to_string(),
            seconds: timeout.as_secs(),
        }),
    }
}

/// Writes the Claude Desktop bundle for the command at `server` into
/// `writ_dir` and answers its path.
pub fn write_bundle_inner(writ_dir: &Path, server: &Path) -> Result<PathBuf, ConnectError> {
    let manifest = bundle_manifest(&server.display().to_string(), env!("CARGO_PKG_VERSION"));
    writ_storage::mcp_bundle::write_bundle(writ_dir, &manifest, BUNDLE_LAUNCHER)
        .map_err(|error| ConnectError::BundleNotWritten(error.to_string()))
}

// --- The connection test ------------------------------------------------------

/// Starts `command`, a `writ mcp`, as a client would, and answers how many
/// tools it lists.
///
/// It sends `initialize`, then the `initialized` notification and
/// `tools/list`, and never calls a tool: the gate sees no call, so the test
/// is never waiting for approval and leaves no line in the log. The server is
/// stopped once it has answered, or once `timeout` has passed.
pub fn test_server_inner(
    mut command: Command,
    timeout: Duration,
) -> Result<ServerTest, ConnectError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| ConnectError::ServerNotStarted(error.to_string()))?;
    let stderr = drain(child.stderr.take());
    let (lines_tx, lines) = mpsc::channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if lines_tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    let Some(mut stdin) = child.stdin.take() else {
        stop(&mut child);
        return Err(ConnectError::ServerNotStarted("no input pipe".to_string()));
    };
    let [initialize, initialized, list] = probe_requests();
    let deadline = Instant::now() + timeout;
    let outcome = (|| {
        send(&mut stdin, &initialize)?;
        loop {
            match next_line(&lines, deadline)? {
                ProbeLine::Initialized => break,
                ProbeLine::Failed(message) => return Err(ConnectError::ServerFailed(message)),
                ProbeLine::Tools(_) | ProbeLine::Other => {}
            }
        }
        send(&mut stdin, &initialized)?;
        send(&mut stdin, &list)?;
        loop {
            match next_line(&lines, deadline)? {
                ProbeLine::Tools(tools) => return Ok(ServerTest { tools }),
                ProbeLine::Failed(message) => return Err(ConnectError::ServerFailed(message)),
                ProbeLine::Initialized | ProbeLine::Other => {}
            }
        }
    })();
    drop(stdin);
    stop(&mut child);
    outcome.map_err(|error| match error {
        ConnectError::ServerEnded(_) => {
            let said = stderr.text();
            ConnectError::ServerEnded(match last_line(&said) {
                said if said.is_empty() => "it gave no reason".to_string(),
                said => said,
            })
        }
        ConnectError::ServerSilent(_) => ConnectError::ServerSilent(timeout.as_secs()),
        other => other,
    })
}

fn send(stdin: &mut impl Write, line: &str) -> Result<(), ConnectError> {
    stdin
        .write_all(line.as_bytes())
        .and_then(|()| stdin.flush())
        .map_err(|_| ConnectError::ServerEnded(String::new()))
}

fn next_line(lines: &mpsc::Receiver<String>, deadline: Instant) -> Result<ProbeLine, ConnectError> {
    let left = deadline.saturating_duration_since(Instant::now());
    match lines.recv_timeout(left) {
        Ok(line) => Ok(read_probe_line(&line)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ConnectError::ServerSilent(0)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ConnectError::ServerEnded(String::new())),
    }
}

// --- A call while the server is off -----------------------------------------

/// The frontend event for the record in `writ_dir`, when there is one.
pub fn off_call_event(writ_dir: &Path) -> Option<WritFrontendEvent> {
    writ_storage::off_calls::read(writ_dir).map(|call| WritFrontendEvent::McpOffCall { call })
}

// --- IPC ------------------------------------------------------------------------

/// The command every client is handed, or why it cannot be handed out yet.
fn ready_server() -> Result<PathBuf, ConnectError> {
    let command = super::activity::resolved_command();
    if command.needs_install || command.needs_move {
        return Err(ConnectError::CommandNotReady);
    }
    Ok(PathBuf::from(command.path))
}

/// IPC: every card, with whether each client's own command was found. Off the
/// main thread: it runs the login shell once.
#[tauri::command]
pub async fn mcp_connect_cards() -> Result<Vec<ClientCard>, String> {
    super::offload::run_detached(super::offload::ERR_COMMAND_STOPPED, || {
        let command = super::activity::resolved_command();
        let ready = !command.needs_install && !command.needs_move;
        Ok(client_cards_inner(
            Path::new(&command.path),
            ready,
            &current_search(),
        ))
    })
    .await
}

/// IPC: runs `client`'s own `mcp add` (Claude Code, Codex, Gemini CLI).
#[tauri::command]
pub async fn mcp_connect_add(client: McpClient) -> Result<AddResult, String> {
    super::offload::run_detached(super::offload::ERR_COMMAND_STOPPED, move || {
        let server = ready_server().map_err(|error| error.to_string())?;
        let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
        add_with_client_inner(client, &server, &current_search(), &home, ADD_TIMEOUT)
            .map_err(|error| error.to_string())
    })
    .await
}

/// IPC: opens `client`'s install link (Cursor, VS Code, LM Studio), or writes
/// the Claude Desktop bundle and hands it to the system.
#[tauri::command]
pub async fn mcp_connect_open(app: AppHandle, client: McpClient) -> Result<(), String> {
    let opener = app.clone();
    super::offload::run_blocking(app, super::offload::ERR_COMMAND_STOPPED, move |state| {
        let server = ready_server().map_err(|error| error.to_string())?;
        open_for_client(&opener, state, client, &server).map_err(|error| error.to_string())
    })
    .await
}

fn open_for_client(
    app: &AppHandle,
    state: &AppState,
    client: McpClient,
    server: &Path,
) -> Result<(), ConnectError> {
    match client.connection() {
        Connection::Link => {
            let link = client
                .install_link(&server.display().to_string())
                .ok_or(ConnectError::WrongConnection)?;
            app.opener().open_url(link, None::<&str>).map_err(|error| {
                tracing::warn!(?client, error = %error, "the install link was not opened");
                ConnectError::LinkNotOpened
            })
        }
        Connection::Bundle => {
            let bundle = write_bundle_inner(&state.writ_dir, server)?;
            app.opener()
                .open_path(bundle.display().to_string(), None::<&str>)
                .map_err(|error| {
                    tracing::warn!(error = %error, "the bundle was not opened");
                    ConnectError::BundleNotOpened
                })
        }
        Connection::Command | Connection::Json => Err(ConnectError::WrongConnection),
    }
}

/// IPC: starts the command every client is handed and answers how many tools
/// it lists.
#[tauri::command]
pub async fn mcp_server_test() -> Result<ServerTest, String> {
    super::offload::run_detached(super::offload::ERR_COMMAND_STOPPED, || {
        let server = ready_server().map_err(|error| error.to_string())?;
        let mut command = Command::new(server);
        command.arg(SERVE_VERB);
        if let Some(home) = dirs::home_dir() {
            command.current_dir(home);
        }
        test_server_inner(command, PROBE_TIMEOUT).map_err(|error| error.to_string())
    })
    .await
}

/// IPC: the last program that called while Connected programs was off.
#[tauri::command]
pub async fn mcp_off_call(app: AppHandle) -> Result<Option<OffCall>, String> {
    super::offload::run_blocking(app, super::offload::ERR_COMMAND_STOPPED, |state| {
        Ok(writ_storage::off_calls::read(&state.writ_dir))
    })
    .await
}
