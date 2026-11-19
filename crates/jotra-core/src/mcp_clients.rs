//! The MCP clients Jotra offers to connect, and what each one is handed
//! (ADR-031 rule 2.4, decision AM-D7).
//!
//! Jotra never edits another program's configuration. A client with a command
//! line of its own (Claude Code, Codex, Gemini CLI) is handed the arguments of
//! its own `mcp add`; Cursor, VS Code and LM Studio are handed the install link
//! each documents; Claude Desktop is handed a desktop extension bundle it
//! installs itself; any other client is handed the JSON most of them read.
//! Every one of them launches the same `<jotra> mcp`.
//!
//! The strings are built here so the shell runs them and the tests pin them
//! in one place: the webview names a client and nothing else.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};

use crate::legacy_names;
use crate::mcp_dialect::{Dialect, COMMAND_NAME_VAR};

/// The name Jotra is registered under in every client.
pub const SERVER_NAME: &str = "jotra";

/// The verb the `jotra` command serves MCP under.
pub const SERVE_VERB: &str = "mcp";

/// The protocol version the connection test asks for.
pub const PROBE_PROTOCOL_VERSION: &str = "2025-06-18";

/// The name the connection test sends as its client. It lists tools and
/// calls none, so the gate never sees it and it is never waiting.
pub const PROBE_CLIENT_NAME: &str = "jotra-connection-test";

/// Whether `name` is the client name of a connection test, from this build or
/// one made before the rename.
pub fn is_probe_client(name: &str) -> bool {
    name == PROBE_CLIENT_NAME || name == legacy_names::PROBE_CLIENT_NAME
}

/// The JSON-RPC id of the connection test's `initialize` request.
pub const PROBE_INITIALIZE_ID: u64 = 1;

/// The JSON-RPC id of the connection test's `tools/list` request.
pub const PROBE_LIST_ID: u64 = 2;

/// The file name of the bundle Claude Desktop installs.
pub const BUNDLE_FILE_NAME: &str = "Jotra.mcpb";

/// Where the bundle's launcher sits inside it.
pub const BUNDLE_LAUNCHER_PATH: &str = "server/index.js";

/// The environment variable the bundle's launcher reads the command from.
pub const BUNDLE_COMMAND_VAR: &str = "JOTRA_COMMAND";

/// The launcher's source, with [`LEGACY_COMMAND_VAR_SLOT`] where the variable
/// an older extension configuration named the command under goes.
const BUNDLE_LAUNCHER_TEMPLATE: &str = include_str!("mcpb_launcher.js");

/// The slot in [`BUNDLE_LAUNCHER_TEMPLATE`] [`bundle_launcher`] fills.
const LEGACY_COMMAND_VAR_SLOT: &str = "{{LEGACY_COMMAND_VAR}}";

/// The launcher a Claude Desktop extension runs on the Node.js Claude Desktop
/// ships: it starts the installed `jotra mcp` on the same pipes, so the
/// extension never holds a copy of the command that goes stale on update. It
/// reads [`BUNDLE_COMMAND_VAR`], then the variable bundles made before the
/// rename set.
pub fn bundle_launcher() -> String {
    BUNDLE_LAUNCHER_TEMPLATE.replace(LEGACY_COMMAND_VAR_SLOT, legacy_names::COMMAND_VAR)
}

/// The manifest version the bundle declares
/// (<https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md>).
pub const BUNDLE_MANIFEST_VERSION: &str = "0.3";

/// `encodeURIComponent`'s set: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// A client Jotra has a card for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpClient {
    /// Anthropic's command-line agent.
    ClaudeCode,
    /// OpenAI's Codex command line, which the ChatGPT app and the IDE
    /// extension share their MCP list with.
    Codex,
    /// Google's Gemini command line.
    GeminiCli,
    /// The Cursor editor.
    Cursor,
    /// Visual Studio Code.
    VsCode,
    /// LM Studio, 0.3.17 or later.
    LmStudio,
    /// Anthropic's desktop app.
    ClaudeDesktop,
    /// Any other client: the JSON to paste.
    Other,
}

/// How a client is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Connection {
    /// The client's own `mcp add`, run when its command is found and copied
    /// otherwise.
    Command,
    /// The install link the client handles.
    Link,
    /// A desktop extension bundle the client installs.
    Bundle,
    /// JSON the person pastes into the client.
    Json,
}

impl McpClient {
    /// Every client, in the order the cards are shown.
    pub const ALL: [McpClient; 8] = [
        McpClient::ClaudeCode,
        McpClient::Codex,
        McpClient::GeminiCli,
        McpClient::Cursor,
        McpClient::VsCode,
        McpClient::LmStudio,
        McpClient::ClaudeDesktop,
        McpClient::Other,
    ];

    /// How this client is connected.
    pub fn connection(self) -> Connection {
        match self {
            McpClient::ClaudeCode | McpClient::Codex | McpClient::GeminiCli => Connection::Command,
            McpClient::Cursor | McpClient::VsCode | McpClient::LmStudio => Connection::Link,
            McpClient::ClaudeDesktop => Connection::Bundle,
            McpClient::Other => Connection::Json,
        }
    }

    /// The command the client's own command line is run as, for the three
    /// that have one.
    pub fn program(self) -> Option<&'static str> {
        match self {
            McpClient::ClaudeCode => Some("claude"),
            McpClient::Codex => Some("codex"),
            McpClient::GeminiCli => Some("gemini"),
            _ => None,
        }
    }

    /// The arguments of the client's own `mcp add` for `server`, the path of
    /// the `jotra` command.
    ///
    /// Claude Code and Gemini CLI are told the user scope: their default is
    /// the folder the command runs in, so a server added there is missing in
    /// every other project (decision MK-D6). Codex has one list for the user.
    pub fn add_args(self, server: &str) -> Option<Vec<String>> {
        let args: Vec<&str> = match self {
            McpClient::ClaudeCode => vec![
                "mcp",
                "add",
                "--scope",
                "user",
                SERVER_NAME,
                "--",
                server,
                SERVE_VERB,
            ],
            McpClient::Codex => vec!["mcp", "add", SERVER_NAME, "--", server, SERVE_VERB],
            McpClient::GeminiCli => vec![
                "mcp",
                "add",
                "--scope",
                "user",
                SERVER_NAME,
                server,
                SERVE_VERB,
            ],
            _ => return None,
        };
        Some(args.into_iter().map(str::to_string).collect())
    }

    /// The `mcp add` line to paste into a terminal, with the path quoted as
    /// the connected-programs command quotes it.
    pub fn add_command_line(self, server: &str) -> Option<String> {
        let program = self.program()?;
        let args = self.add_args(server)?;
        let words: Vec<String> = args
            .into_iter()
            .map(|arg| {
                if arg == server {
                    format!("\"{arg}\"")
                } else {
                    arg
                }
            })
            .collect();
        Some(format!("{program} {}", words.join(" ")))
    }

    /// The install link for the three clients that take one, with the
    /// configuration encoded the way each documents it:
    ///
    /// - Cursor: `cursor://anysphere.cursor-deeplink/mcp/install?name=…&config=…`,
    ///   the config base64 of the JSON (<https://cursor.com/docs/context/mcp/install-links>).
    /// - VS Code: `vscode:mcp/install?…`, `encodeURIComponent` of the JSON with
    ///   its name inside (<https://code.visualstudio.com/api/extension-guides/ai/mcp>).
    /// - LM Studio: `lmstudio://add_mcp?name=…&config=…`, the config base64 of
    ///   the JSON (<https://lmstudio.ai/docs/app/mcp/deeplink>).
    ///
    /// Base64 is percent-encoded in the query, as LM Studio's example is: its
    /// `+`, `/` and `=` mean something else there.
    pub fn install_link(self, server: &str) -> Option<String> {
        match self {
            McpClient::Cursor => Some(format!(
                "cursor://anysphere.cursor-deeplink/mcp/install?name={SERVER_NAME}&config={}",
                encode_component(&base64_json(server))
            )),
            McpClient::VsCode => Some(format!(
                "vscode:mcp/install?{}",
                encode_component(&named_config_json(server))
            )),
            McpClient::LmStudio => Some(format!(
                "lmstudio://add_mcp?name={SERVER_NAME}&config={}",
                encode_component(&base64_json(server))
            )),
            _ => None,
        }
    }
}

/// One stdio server, as every JSON client reads it.
#[derive(Debug, Serialize)]
struct StdioServer<'a> {
    command: &'a str,
    args: [&'a str; 1],
}

/// [`StdioServer`] with its name inside, as VS Code's link takes it.
#[derive(Debug, Serialize)]
struct NamedStdioServer<'a> {
    name: &'a str,
    command: &'a str,
    args: [&'a str; 1],
}

/// The server's configuration as compact JSON: `{"command":…,"args":["mcp"]}`.
pub fn config_json(server: &str) -> String {
    serde_json::to_string(&StdioServer {
        command: server,
        args: [SERVE_VERB],
    })
    .unwrap_or_default()
}

fn named_config_json(server: &str) -> String {
    serde_json::to_string(&NamedStdioServer {
        name: SERVER_NAME,
        command: server,
        args: [SERVE_VERB],
    })
    .unwrap_or_default()
}

fn base64_json(server: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(config_json(server))
}

/// `encodeURIComponent`.
pub fn encode_component(text: &str) -> String {
    utf8_percent_encode(text, URI_COMPONENT).to_string()
}

/// The `mcpServers` block most JSON clients read (Claude Desktop's own file,
/// Cursor's, Windsurf's and others), indented for a person to paste.
pub fn other_client_json(server: &str) -> String {
    let servers = std::collections::BTreeMap::from([(
        SERVER_NAME,
        StdioServer {
            command: server,
            args: [SERVE_VERB],
        },
    )]);
    let block = serde_json::json!({ "mcpServers": servers });
    serde_json::to_string_pretty(&block).unwrap_or_default()
}

/// The `manifest.json` of the bundle Claude Desktop installs.
///
/// A Node.js server whose entry point is [`bundle_launcher`]: Claude Desktop
/// runs `node` on it with [`BUNDLE_COMMAND_VAR`] set to `server`, and the
/// launcher starts `server mcp`. `version` is the app's.
///
/// The manifest keeps the extension name bundles carried before the rename,
/// so an installed extension updates in place and keeps its tool settings.
/// [`COMMAND_NAME_VAR`] names the command after it, so the server answers in
/// the dialect those settings were made under.
pub fn bundle_manifest(server: &str, version: &str) -> String {
    let extension_name = legacy_names::BUNDLE_NAME;
    let command_name = legacy_names::COMMAND_NAME;
    let manifest = serde_json::json!({
        "manifest_version": BUNDLE_MANIFEST_VERSION,
        "name": extension_name,
        "display_name": "Jotra",
        "version": version,
        "description": "Reads the files in your Jotra folder, and writes them if you allow it in Jotra.",
        "author": { "name": "Jotra" },
        "homepage": "https://writ.ibrahemid.com",
        "server": {
            "type": "node",
            "entry_point": BUNDLE_LAUNCHER_PATH,
            "mcp_config": {
                "command": "node",
                "args": [format!("${{__dirname}}/{BUNDLE_LAUNCHER_PATH}")],
                "env": { BUNDLE_COMMAND_VAR: server, COMMAND_NAME_VAR: command_name },
            },
        },
        "compatibility": {
            "platforms": ["darwin", "win32", "linux"],
        },
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

/// What a client's entry for a server reads as on the connected-programs row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistrationStatus {
    /// The entry carries a name the server answers under, and its command is a
    /// build of the `jotra` command.
    Connected,
    /// The entry carries a name the server answers under, and its command is
    /// gone or is another program.
    Broken,
    /// The entry is another server's.
    Unrelated,
}

/// Whether `name` is a name the server is registered under: the current one,
/// or the one registrations made before the rename carry.
pub fn is_server_name(name: &str) -> bool {
    Dialect::ALL
        .iter()
        .any(|dialect| dialect.server_name() == name)
}

/// What an entry named `name` reads as, given whether the command it launches
/// resolves to a file carrying [`crate::cli_install::CLI_SIGNATURE`].
pub fn registration_status(name: &str, command_is_jotra: bool) -> RegistrationStatus {
    match (is_server_name(name), command_is_jotra) {
        (false, _) => RegistrationStatus::Unrelated,
        (true, true) => RegistrationStatus::Connected,
        (true, false) => RegistrationStatus::Broken,
    }
}

/// The command a repair points an entry named `name` at, given
/// `canonical_command`, where the `jotra` command is installed.
///
/// The repair keeps the entry's name and picks the command whose file name
/// equals it, beside the canonical one and with its suffix, so the server it
/// starts answers in the dialect the entry was approved under. `None` for an
/// entry that is not the server's.
pub fn repair_command(name: &str, canonical_command: &Path) -> Option<PathBuf> {
    if !is_server_name(name) {
        return None;
    }
    let file_name = match canonical_command.extension() {
        Some(extension) => format!("{name}.{}", extension.to_string_lossy()),
        None => name.to_string(),
    };
    Some(canonical_command.with_file_name(file_name))
}

/// The three lines the connection test writes, in order: `initialize`, the
/// `initialized` notification and `tools/list`, each newline-terminated. The
/// last two go once the first is answered.
pub fn probe_requests() -> [String; 3] {
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": PROBE_INITIALIZE_ID,
        "method": "initialize",
        "params": {
            "protocolVersion": PROBE_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": PROBE_CLIENT_NAME, "version": env!("CARGO_PKG_VERSION") },
        },
    });
    let initialized =
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let list = serde_json::json!({
        "jsonrpc": "2.0",
        "id": PROBE_LIST_ID,
        "method": "tools/list",
        "params": {},
    });
    [initialize, initialized, list].map(|message| format!("{message}\n"))
}

/// What one line the server wrote says about the connection test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeLine {
    /// The answer to `initialize`.
    Initialized,
    /// The answer to `tools/list`, with the number of tools it listed.
    Tools(usize),
    /// The server answered a request with an error, and this was its message.
    Failed(String),
    /// Anything else: a notification, a log line.
    Other,
}

/// Reads one line of the server's output.
pub fn read_probe_line(line: &str) -> ProbeLine {
    let Ok(message) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return ProbeLine::Other;
    };
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("no message")
            .to_string();
        return ProbeLine::Failed(text);
    }
    match message.get("id").and_then(serde_json::Value::as_u64) {
        Some(PROBE_INITIALIZE_ID) if message.get("result").is_some() => {
            return ProbeLine::Initialized
        }
        Some(PROBE_LIST_ID) => {}
        _ => return ProbeLine::Other,
    }
    match message
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(serde_json::Value::as_array)
    {
        Some(tools) => ProbeLine::Tools(tools.len()),
        None => ProbeLine::Failed("the tool list was missing from the answer".to_string()),
    }
}

/// The marker the login shell prints around its `PATH`, so whatever its
/// startup files print is told apart from the value.
pub const PATH_MARKER: &str = "__JOTRA_PATH_7f3a__";

/// The command a login shell runs to print its `PATH` between two
/// [`PATH_MARKER`]s.
pub fn login_shell_path_script() -> String {
    format!("printf '%s%s%s' '{PATH_MARKER}' \"$PATH\" '{PATH_MARKER}'")
}

/// The `PATH` a login shell printed, read from between the markers.
pub fn read_login_shell_path(output: &str) -> Option<String> {
    let start = output.find(PATH_MARKER)? + PATH_MARKER.len();
    let rest = &output[start..];
    let end = rest.find(PATH_MARKER)?;
    let path = rest[..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// Folders a client's command is installed into that a desktop app's own
/// `PATH` usually lacks, relative to the home folder or absolute: the native
/// Claude Code install, npm, Bun, Volta and Homebrew. Looked in after the
/// login shell's `PATH` and the app's own.
pub const USUAL_PROGRAM_FOLDERS: &[&str] = &[
    ".local/bin",
    ".claude/local",
    ".npm-global/bin",
    ".bun/bin",
    ".volta/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
];
