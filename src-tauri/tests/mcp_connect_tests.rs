//! Connecting an MCP client from its card (ADR-031 rule 2.4): the cards, a
//! client's own `mcp add`, the Claude Desktop bundle and the connection test,
//! each through its Tauri-free inner function.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use writ_core::mcp_clients::{Connection, McpClient};
use writ_tauri_lib::commands::mcp_connect::{
    client_cards_inner, off_call_event, test_server_inner, write_bundle_inner, ConnectError,
    ProgramSearch,
};
use writ_tauri_lib::events::emitter::WritFrontendEvent;

const LIB_RS: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "commands::mcp_connect::mcp_connect_cards",
    "commands::mcp_connect::mcp_connect_add",
    "commands::mcp_connect::mcp_connect_open",
    "commands::mcp_connect::mcp_server_test",
    "commands::mcp_connect::mcp_off_call",
];

const SERVER: &str = "/Applications/Writ.app/Contents/MacOS/writ";

/// A search with no folders, so no program on this machine is found.
fn nowhere() -> ProgramSearch {
    ProgramSearch {
        folders: Vec::new(),
        path: OsString::new(),
    }
}

/// The `writ` command this test run built, beside the test binary's folder.
fn built_writ() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let debug = exe.parent().and_then(Path::parent).expect("target/debug");
    debug.join(if cfg!(windows) { "writ.exe" } else { "writ" })
}

#[test]
fn every_command_is_in_the_invoke_handler() {
    for command in COMMANDS {
        assert!(
            LIB_RS.contains(command),
            "{command} is not in the invoke handler"
        );
    }
}

#[test]
fn every_client_has_a_card_in_order() {
    let cards = client_cards_inner(Path::new(SERVER), true, &nowhere());
    let clients: Vec<McpClient> = cards.iter().map(|card| card.client).collect();
    assert_eq!(clients, McpClient::ALL);
}

#[test]
fn a_command_card_copies_the_clients_own_add_line() {
    let cards = client_cards_inner(Path::new(SERVER), true, &nowhere());
    let claude = cards
        .iter()
        .find(|card| card.client == McpClient::ClaudeCode)
        .expect("a card");
    assert_eq!(claude.connection, Connection::Command);
    assert_eq!(
        claude.copy_text.as_deref(),
        Some("claude mcp add --scope user writ -- \"/Applications/Writ.app/Contents/MacOS/writ\" mcp")
    );
    assert!(!claude.runs_command, "no folder was searched");
}

#[test]
fn the_other_card_copies_the_json_and_the_link_cards_copy_nothing() {
    let cards = client_cards_inner(Path::new(SERVER), true, &nowhere());
    for card in &cards {
        match card.connection {
            Connection::Json => {
                let json: serde_json::Value =
                    serde_json::from_str(card.copy_text.as_deref().expect("json")).expect("parse");
                assert_eq!(json["mcpServers"]["writ"]["command"], SERVER);
            }
            Connection::Link | Connection::Bundle => assert!(card.copy_text.is_none()),
            Connection::Command => assert!(card.copy_text.is_some()),
        }
    }
}

#[test]
fn a_command_that_is_not_ready_is_handed_to_no_card() {
    let cards = client_cards_inner(Path::new(SERVER), false, &nowhere());
    for card in cards {
        assert!(card.copy_text.is_none(), "{:?}", card.client);
        assert!(!card.runs_command, "{:?}", card.client);
    }
}

/// The `PATH` separator and an absolute path are spelled the Unix way here.
#[cfg(unix)]
#[test]
fn the_search_tries_the_login_shell_then_the_app_then_the_usual_folders() {
    use writ_tauri_lib::commands::mcp_connect::program_search;

    let search = program_search(
        Some("/opt/homebrew/bin:/usr/bin"),
        Some(std::ffi::OsStr::new("/usr/bin:/bin")),
        Some(Path::new("/Users/someone")),
    );
    let first: Vec<&Path> = search
        .folders
        .iter()
        .take(3)
        .map(PathBuf::as_path)
        .collect();
    assert_eq!(
        first,
        [
            Path::new("/opt/homebrew/bin"),
            Path::new("/usr/bin"),
            Path::new("/bin")
        ]
    );
    assert!(search
        .folders
        .contains(&PathBuf::from("/Users/someone/.local/bin")));
    assert_eq!(
        search
            .folders
            .iter()
            .filter(|folder| folder.as_path() == Path::new("/opt/homebrew/bin"))
            .count(),
        1,
        "a folder is tried once"
    );
    let path: Vec<PathBuf> = std::env::split_paths(&search.path).collect();
    assert_eq!(path, search.folders);
}

/// The `PATH` separator and an absolute path are spelled the Unix way here.
#[cfg(unix)]
#[test]
fn a_relative_folder_in_a_path_is_never_searched() {
    use writ_tauri_lib::commands::mcp_connect::program_search;

    let search = program_search(Some("bin:/usr/bin"), None, None);
    assert!(search.folders.iter().all(|folder| folder.is_absolute()));
}

#[test]
fn the_bundle_is_written_into_the_data_folder_and_names_the_command() {
    let dir = tempfile::TempDir::new().expect("temp dir");

    let bundle = write_bundle_inner(dir.path(), Path::new(SERVER)).expect("written");

    assert!(bundle.starts_with(dir.path()));
    let bytes = std::fs::read(&bundle).expect("read");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("\"WRIT_COMMAND\": \"/Applications/Writ.app/Contents/MacOS/writ\""));
    assert!(text.contains(concat!("\"version\": \"", env!("CARGO_PKG_VERSION"), "\"")));
}

#[test]
fn a_recorded_call_while_off_becomes_the_offer_event() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    assert!(off_call_event(dir.path()).is_none());

    writ_storage::off_calls::note(
        dir.path(),
        &writ_core::activity::ClientId::named("claude-code"),
    )
    .expect("written");

    match off_call_event(dir.path()) {
        Some(WritFrontendEvent::McpOffCall { call }) => assert_eq!(call.name, "claude-code"),
        other => panic!("expected the offer event, got {other:?}"),
    }
}

/// A data folder and a notes folder for a `writ mcp` started by a test.
fn server_folders() -> (tempfile::TempDir, Command) {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let data = dir.path().join("data");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&data).expect("data folder");
    std::fs::create_dir_all(&notes).expect("notes folder");
    std::fs::write(notes.join("Plan.md"), "# Plan\n").expect("a file");
    let mut command = Command::new(built_writ());
    command
        .arg("mcp")
        .env("WRIT_DATA_DIR", &data)
        .env("WRIT_NOTES_DIR", &notes);
    (dir, command)
}

#[test]
fn the_test_counts_the_tools_the_built_server_lists() {
    let (dir, command) = server_folders();

    let answer = test_server_inner(command, Duration::from_secs(20)).expect("an answer");

    let expected = writ_core::tools::tool_names().len();
    assert_eq!(answer.tools, expected);
    let data = dir.path().join("data");
    assert!(
        !writ_storage::pending_clients::path(&data).exists(),
        "the test is never waiting for approval"
    );
    assert!(
        writ_storage::activity_log::read_recent(&data, 10).is_empty(),
        "the test leaves no line in the log"
    );
}

#[test]
fn a_server_that_cannot_be_started_says_so() {
    let command = Command::new("/nonexistent/writ");
    let error = test_server_inner(command, Duration::from_secs(2)).expect_err("no server");
    assert!(matches!(error, ConnectError::ServerNotStarted(_)));
}

#[cfg(unix)]
#[test]
fn a_server_that_ends_early_passes_on_what_it_said() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "echo 'the folder could not be read' >&2; exit 3"]);

    let error = test_server_inner(command, Duration::from_secs(5)).expect_err("no tools");

    assert_eq!(
        error,
        ConnectError::ServerEnded("the folder could not be read".to_string())
    );
}

#[cfg(unix)]
#[test]
fn a_server_that_never_answers_is_stopped_at_the_deadline() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "cat > /dev/null"]);
    let started = std::time::Instant::now();

    let error = test_server_inner(command, Duration::from_millis(400)).expect_err("silent");

    assert!(matches!(error, ConnectError::ServerSilent(_)));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
mod a_clients_own_command {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use writ_tauri_lib::commands::mcp_connect::{add_with_client_inner, find_program};

    /// A folder holding an executable `name` that runs `body`.
    fn program(folder: &Path, name: &str, body: &str) -> PathBuf {
        let path = folder.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// A search that holds `folder` and no other.
    fn search_in(folder: &Path) -> ProgramSearch {
        ProgramSearch {
            folders: vec![folder.to_path_buf()],
            path: std::env::join_paths([folder]).expect("a folder fit for PATH"),
        }
    }

    #[test]
    fn a_program_that_can_be_run_is_found_and_one_that_cannot_is_not() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        program(dir.path(), "claude", "exit 0");
        std::fs::write(dir.path().join("codex"), "not runnable").expect("file");

        let search = search_in(dir.path());

        assert_eq!(
            find_program("claude", &search),
            Some(dir.path().join("claude"))
        );
        assert_eq!(find_program("codex", &search), None);
        assert_eq!(find_program("gemini", &search), None);
    }

    #[test]
    fn a_found_command_makes_the_card_run_it() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        program(dir.path(), "codex", "exit 0");

        let cards = client_cards_inner(Path::new(SERVER), true, &search_in(dir.path()));

        let runs: Vec<McpClient> = cards
            .iter()
            .filter(|card| card.runs_command)
            .map(|card| card.client)
            .collect();
        assert_eq!(runs, [McpClient::Codex]);
    }

    #[test]
    fn claude_code_is_run_with_its_add_arguments_from_the_home_folder() {
        let bin = tempfile::TempDir::new().expect("bin");
        let home = tempfile::TempDir::new().expect("home");
        let record = home.path().join("record.txt");
        program(
            bin.path(),
            "claude",
            &format!(
                "printf '%s\\n' \"$@\" > '{}'; pwd >> '{}'; echo 'Added stdio MCP server writ'",
                record.display(),
                record.display()
            ),
        );

        let added = add_with_client_inner(
            McpClient::ClaudeCode,
            Path::new(SERVER),
            &search_in(bin.path()),
            home.path(),
            Duration::from_secs(10),
        )
        .expect("added");

        assert_eq!(added.said, "Added stdio MCP server writ");
        let recorded = std::fs::read_to_string(&record).expect("record");
        let lines: Vec<&str> = recorded.lines().collect();
        assert_eq!(
            &lines[..8],
            ["mcp", "add", "--scope", "user", "writ", "--", SERVER, "mcp"]
        );
        let ran_in = std::fs::canonicalize(lines[8]).expect("cwd");
        assert_eq!(ran_in, std::fs::canonicalize(home.path()).expect("home"));
    }

    #[test]
    fn a_client_that_fails_passes_on_what_it_said() {
        let bin = tempfile::TempDir::new().expect("bin");
        let home = tempfile::TempDir::new().expect("home");
        program(
            bin.path(),
            "gemini",
            "echo 'MCP server writ already exists in user settings.' >&2; exit 1",
        );

        let error = add_with_client_inner(
            McpClient::GeminiCli,
            Path::new(SERVER),
            &search_in(bin.path()),
            home.path(),
            Duration::from_secs(10),
        )
        .expect_err("failed");

        assert_eq!(
            error,
            ConnectError::ProgramFailed {
                program: "gemini".to_string(),
                said: "MCP server writ already exists in user settings".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "The gemini command could not add Writ: MCP server writ already exists in user settings."
        );
    }

    #[test]
    fn a_client_that_hangs_is_stopped() {
        let bin = tempfile::TempDir::new().expect("bin");
        let home = tempfile::TempDir::new().expect("home");
        program(bin.path(), "codex", "exec /bin/sleep 30");

        let error = add_with_client_inner(
            McpClient::Codex,
            Path::new(SERVER),
            &search_in(bin.path()),
            home.path(),
            Duration::from_millis(300),
        )
        .expect_err("timed out");

        assert!(
            matches!(error, ConnectError::ProgramTimedOut { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_client_that_leaves_a_process_holding_its_output_still_finishes() {
        let bin = tempfile::TempDir::new().expect("bin");
        let home = tempfile::TempDir::new().expect("home");
        program(bin.path(), "claude", "/bin/sleep 30 & echo 'Added'");
        let started = std::time::Instant::now();

        let added = add_with_client_inner(
            McpClient::ClaudeCode,
            Path::new(SERVER),
            &search_in(bin.path()),
            home.path(),
            Duration::from_secs(10),
        );

        assert!(added.is_ok(), "{added:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_client_that_is_not_installed_is_named() {
        let empty = tempfile::TempDir::new().expect("empty");
        let error = add_with_client_inner(
            McpClient::ClaudeCode,
            Path::new(SERVER),
            &search_in(empty.path()),
            empty.path(),
            Duration::from_secs(1),
        )
        .expect_err("missing");
        assert_eq!(
            error,
            ConnectError::ProgramNotFound {
                program: "claude".to_string()
            }
        );
    }

    #[test]
    fn a_link_client_is_never_run() {
        let empty = tempfile::TempDir::new().expect("empty");
        let error = add_with_client_inner(
            McpClient::Cursor,
            Path::new(SERVER),
            &search_in(empty.path()),
            empty.path(),
            Duration::from_secs(1),
        )
        .expect_err("not a command client");
        assert_eq!(error, ConnectError::WrongConnection);
    }
}

/// The bundle's launcher, run on Node as Claude Desktop runs it, starts the
/// built server and the test reaches its tool list through it.
#[test]
fn the_bundle_launcher_starts_the_command_it_is_given() {
    let node_found = Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !node_found {
        eprintln!("node is not on this machine's PATH; the launcher was not run");
        return;
    }
    let dir = tempfile::TempDir::new().expect("temp dir");
    let launcher = dir.path().join("index.js");
    std::fs::write(&launcher, writ_core::mcp_clients::BUNDLE_LAUNCHER).expect("launcher");
    let (folders, server) = server_folders();
    let mut command = Command::new("node");
    command
        .arg(&launcher)
        .env("WRIT_COMMAND", server.get_program());
    for (key, value) in server.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }

    let answer = test_server_inner(command, Duration::from_secs(20)).expect("an answer");

    assert!(answer.tools > 0);
    drop(folders);
}
