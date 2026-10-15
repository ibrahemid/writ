//! The command, link, bundle and JSON each MCP client is handed (ADR-031 rule
//! 2.4). The expected links were generated with Node from each client's
//! documented recipe, so a change to the encoding fails here rather than in a
//! client:
//!
//! ```js
//! const config = JSON.stringify({ command: path, args: ["mcp"] });
//! const b64 = Buffer.from(config, "utf8").toString("base64");
//! `cursor://anysphere.cursor-deeplink/mcp/install?name=writ&config=${encodeURIComponent(b64)}`;
//! `vscode:mcp/install?${encodeURIComponent(JSON.stringify({ name: "writ", command: path, args: ["mcp"] }))}`;
//! `lmstudio://add_mcp?name=writ&config=${encodeURIComponent(b64)}`;
//! ```

use base64::Engine as _;
use writ_core::mcp_clients::{
    bundle_manifest, config_json, encode_component, login_shell_path_script, other_client_json,
    probe_requests, read_login_shell_path, read_probe_line, Connection, McpClient, ProbeLine,
    BUNDLE_COMMAND_VAR, BUNDLE_LAUNCHER, BUNDLE_LAUNCHER_PATH, PATH_MARKER, PROBE_CLIENT_NAME,
    PROBE_LIST_ID, SERVER_NAME,
};

const APP: &str = "/Applications/Writ.app/Contents/MacOS/writ";
const SPACED: &str = "/Users/a b/Apps/Writ.app/Contents/MacOS/writ";
const ACCENTED: &str = "/Users/ré/Écrit/Writ.app/Contents/MacOS/writ";
const WINDOWS: &str = "C:\\Program Files\\Writ\\writ.exe";

struct Expected {
    path: &'static str,
    cursor: &'static str,
    vscode: &'static str,
    lmstudio: &'static str,
}

const LINKS: &[Expected] = &[
    Expected {
        path: APP,
        cursor: "cursor://anysphere.cursor-deeplink/mcp/install?name=writ&config=eyJjb21tYW5kIjoiL0FwcGxpY2F0aW9ucy9Xcml0LmFwcC9Db250ZW50cy9NYWNPUy93cml0IiwiYXJncyI6WyJtY3AiXX0%3D",
        vscode: "vscode:mcp/install?%7B%22name%22%3A%22writ%22%2C%22command%22%3A%22%2FApplications%2FWrit.app%2FContents%2FMacOS%2Fwrit%22%2C%22args%22%3A%5B%22mcp%22%5D%7D",
        lmstudio: "lmstudio://add_mcp?name=writ&config=eyJjb21tYW5kIjoiL0FwcGxpY2F0aW9ucy9Xcml0LmFwcC9Db250ZW50cy9NYWNPUy93cml0IiwiYXJncyI6WyJtY3AiXX0%3D",
    },
    Expected {
        path: SPACED,
        cursor: "cursor://anysphere.cursor-deeplink/mcp/install?name=writ&config=eyJjb21tYW5kIjoiL1VzZXJzL2EgYi9BcHBzL1dyaXQuYXBwL0NvbnRlbnRzL01hY09TL3dyaXQiLCJhcmdzIjpbIm1jcCJdfQ%3D%3D",
        vscode: "vscode:mcp/install?%7B%22name%22%3A%22writ%22%2C%22command%22%3A%22%2FUsers%2Fa%20b%2FApps%2FWrit.app%2FContents%2FMacOS%2Fwrit%22%2C%22args%22%3A%5B%22mcp%22%5D%7D",
        lmstudio: "lmstudio://add_mcp?name=writ&config=eyJjb21tYW5kIjoiL1VzZXJzL2EgYi9BcHBzL1dyaXQuYXBwL0NvbnRlbnRzL01hY09TL3dyaXQiLCJhcmdzIjpbIm1jcCJdfQ%3D%3D",
    },
    Expected {
        path: ACCENTED,
        cursor: "cursor://anysphere.cursor-deeplink/mcp/install?name=writ&config=eyJjb21tYW5kIjoiL1VzZXJzL3LDqS%2FDiWNyaXQvV3JpdC5hcHAvQ29udGVudHMvTWFjT1Mvd3JpdCIsImFyZ3MiOlsibWNwIl19",
        vscode: "vscode:mcp/install?%7B%22name%22%3A%22writ%22%2C%22command%22%3A%22%2FUsers%2Fr%C3%A9%2F%C3%89crit%2FWrit.app%2FContents%2FMacOS%2Fwrit%22%2C%22args%22%3A%5B%22mcp%22%5D%7D",
        lmstudio: "lmstudio://add_mcp?name=writ&config=eyJjb21tYW5kIjoiL1VzZXJzL3LDqS%2FDiWNyaXQvV3JpdC5hcHAvQ29udGVudHMvTWFjT1Mvd3JpdCIsImFyZ3MiOlsibWNwIl19",
    },
    Expected {
        path: WINDOWS,
        cursor: "cursor://anysphere.cursor-deeplink/mcp/install?name=writ&config=eyJjb21tYW5kIjoiQzpcXFByb2dyYW0gRmlsZXNcXFdyaXRcXHdyaXQuZXhlIiwiYXJncyI6WyJtY3AiXX0%3D",
        vscode: "vscode:mcp/install?%7B%22name%22%3A%22writ%22%2C%22command%22%3A%22C%3A%5C%5CProgram%20Files%5C%5CWrit%5C%5Cwrit.exe%22%2C%22args%22%3A%5B%22mcp%22%5D%7D",
        lmstudio: "lmstudio://add_mcp?name=writ&config=eyJjb21tYW5kIjoiQzpcXFByb2dyYW0gRmlsZXNcXFdyaXRcXHdyaXQuZXhlIiwiYXJncyI6WyJtY3AiXX0%3D",
    },
];

#[test]
fn the_cursor_link_is_base64_of_the_json_as_cursor_documents() {
    for expected in LINKS {
        assert_eq!(
            McpClient::Cursor.install_link(expected.path).as_deref(),
            Some(expected.cursor),
            "{}",
            expected.path
        );
    }
}

#[test]
fn the_vs_code_link_is_the_url_encoded_json_with_its_name_inside() {
    for expected in LINKS {
        assert_eq!(
            McpClient::VsCode.install_link(expected.path).as_deref(),
            Some(expected.vscode),
            "{}",
            expected.path
        );
    }
}

#[test]
fn the_lm_studio_link_is_base64_of_the_json_url_encoded() {
    for expected in LINKS {
        assert_eq!(
            McpClient::LmStudio.install_link(expected.path).as_deref(),
            Some(expected.lmstudio),
            "{}",
            expected.path
        );
    }
}

#[test]
fn a_link_decodes_back_to_the_command_and_the_verb() {
    let link = McpClient::Cursor.install_link(ACCENTED).expect("a link");
    let encoded = link.split("config=").nth(1).expect("a config");
    let base64 = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .expect("utf-8");
    let json = base64::engine::general_purpose::STANDARD
        .decode(base64.as_bytes())
        .expect("base64");
    let value: serde_json::Value = serde_json::from_slice(&json).expect("json");
    assert_eq!(value["command"], ACCENTED);
    assert_eq!(value["args"], serde_json::json!(["mcp"]));
}

#[test]
fn the_config_keeps_command_before_args() {
    assert_eq!(
        config_json(APP),
        r#"{"command":"/Applications/Writ.app/Contents/MacOS/writ","args":["mcp"]}"#
    );
}

#[test]
fn encode_component_leaves_what_encode_uri_component_leaves() {
    assert_eq!(encode_component("AZaz09-_.!~*'()"), "AZaz09-_.!~*'()");
    assert_eq!(encode_component("a b/c+d=e?&"), "a%20b%2Fc%2Bd%3De%3F%26");
    assert_eq!(encode_component("é"), "%C3%A9");
}

#[test]
fn claude_code_is_added_for_every_project() {
    assert_eq!(
        McpClient::ClaudeCode.add_args(APP).expect("args"),
        ["mcp", "add", "--scope", "user", "writ", "--", APP, "mcp"]
    );
    assert_eq!(
        McpClient::ClaudeCode.add_command_line(SPACED).as_deref(),
        Some("claude mcp add --scope user writ -- \"/Users/a b/Apps/Writ.app/Contents/MacOS/writ\" mcp")
    );
}

#[test]
fn codex_is_added_with_its_own_command() {
    assert_eq!(
        McpClient::Codex.add_args(APP).expect("args"),
        ["mcp", "add", "writ", "--", APP, "mcp"]
    );
    assert_eq!(
        McpClient::Codex.add_command_line(APP).as_deref(),
        Some("codex mcp add writ -- \"/Applications/Writ.app/Contents/MacOS/writ\" mcp")
    );
}

#[test]
fn gemini_cli_is_added_for_the_user_rather_than_the_folder_it_runs_in() {
    assert_eq!(
        McpClient::GeminiCli.add_args(APP).expect("args"),
        ["mcp", "add", "--scope", "user", "writ", APP, "mcp"]
    );
    assert_eq!(
        McpClient::GeminiCli.add_command_line(APP).as_deref(),
        Some("gemini mcp add --scope user writ \"/Applications/Writ.app/Contents/MacOS/writ\" mcp")
    );
}

#[test]
fn every_client_has_one_way_to_connect() {
    for client in McpClient::ALL {
        match client.connection() {
            Connection::Command => {
                assert!(client.program().is_some(), "{client:?}");
                assert!(client.add_args(APP).is_some(), "{client:?}");
                assert!(client.install_link(APP).is_none(), "{client:?}");
            }
            Connection::Link => {
                assert!(client.add_args(APP).is_none(), "{client:?}");
                assert!(client.install_link(APP).is_some(), "{client:?}");
            }
            Connection::Bundle | Connection::Json => {
                assert!(client.add_args(APP).is_none(), "{client:?}");
                assert!(client.install_link(APP).is_none(), "{client:?}");
            }
        }
    }
}

#[test]
fn clients_cross_to_the_app_by_a_kebab_case_id() {
    let ids: Vec<String> = McpClient::ALL
        .iter()
        .map(|client| {
            serde_json::to_value(client)
                .expect("id")
                .as_str()
                .expect("a string")
                .to_string()
        })
        .collect();
    assert_eq!(
        ids,
        [
            "claude-code",
            "codex",
            "gemini-cli",
            "cursor",
            "vs-code",
            "lm-studio",
            "claude-desktop",
            "other"
        ]
    );
    let back: McpClient = serde_json::from_str("\"lm-studio\"").expect("parse");
    assert_eq!(back, McpClient::LmStudio);
}

#[test]
fn the_other_json_is_the_mcp_servers_block() {
    let value: serde_json::Value = serde_json::from_str(&other_client_json(WINDOWS)).expect("json");
    assert_eq!(value["mcpServers"][SERVER_NAME]["command"], WINDOWS);
    assert_eq!(
        value["mcpServers"][SERVER_NAME]["args"],
        serde_json::json!(["mcp"])
    );
}

#[test]
fn the_bundle_runs_the_installed_command_through_its_launcher() {
    let manifest: serde_json::Value =
        serde_json::from_str(&bundle_manifest(SPACED, "0.7.0")).expect("json");
    for field in ["manifest_version", "name", "version", "description"] {
        assert!(manifest[field].is_string(), "{field}");
    }
    assert!(manifest["author"]["name"].is_string());
    assert_eq!(manifest["version"], "0.7.0");
    assert_eq!(manifest["server"]["type"], "node");
    assert_eq!(manifest["server"]["entry_point"], BUNDLE_LAUNCHER_PATH);
    assert_eq!(manifest["server"]["mcp_config"]["command"], "node");
    assert_eq!(
        manifest["server"]["mcp_config"]["args"],
        serde_json::json!(["${__dirname}/server/index.js"])
    );
    assert_eq!(
        manifest["server"]["mcp_config"]["env"][BUNDLE_COMMAND_VAR],
        SPACED
    );
}

#[test]
fn the_launcher_reads_the_command_from_the_variable_the_manifest_sets() {
    assert!(BUNDLE_LAUNCHER.contains(&format!("process.env.{BUNDLE_COMMAND_VAR}")));
    assert!(BUNDLE_LAUNCHER.contains("[\"mcp\"]"));
    assert!(BUNDLE_LAUNCHER.contains("stdio: \"inherit\""));
}

#[test]
fn the_test_initialises_lists_the_tools_and_calls_none() {
    let lines = probe_requests();
    let methods: Vec<String> = lines
        .iter()
        .map(|line| {
            assert!(line.ends_with('\n'));
            let value: serde_json::Value = serde_json::from_str(line.trim()).expect("json");
            value["method"].as_str().expect("a method").to_string()
        })
        .collect();
    assert_eq!(
        methods,
        ["initialize", "notifications/initialized", "tools/list"]
    );
    let initialize: serde_json::Value = serde_json::from_str(lines[0].trim()).expect("json");
    assert_eq!(
        initialize["params"]["clientInfo"]["name"],
        PROBE_CLIENT_NAME
    );
    let list: serde_json::Value = serde_json::from_str(lines[2].trim()).expect("json");
    assert_eq!(list["id"], PROBE_LIST_ID);
}

#[test]
fn the_tool_list_answer_is_counted() {
    let answer =
        r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"a"},{"name":"b"},{"name":"c"}]}}"#;
    assert_eq!(read_probe_line(answer), ProbeLine::Tools(3));
}

#[test]
fn the_initialize_answer_is_told_apart() {
    let initialised = r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"writ"}}}"#;
    assert_eq!(read_probe_line(initialised), ProbeLine::Initialized);
}

#[test]
fn other_lines_are_passed_over() {
    let notification = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{}}"#;
    assert_eq!(read_probe_line(notification), ProbeLine::Other);
    assert_eq!(read_probe_line("not json"), ProbeLine::Other);
    assert_eq!(read_probe_line(""), ProbeLine::Other);
}

#[test]
fn an_error_answer_carries_its_message() {
    let answer = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"Method not found"}}"#;
    assert_eq!(
        read_probe_line(answer),
        ProbeLine::Failed("Method not found".to_string())
    );
    let empty = r#"{"jsonrpc":"2.0","id":2,"result":{}}"#;
    assert!(matches!(read_probe_line(empty), ProbeLine::Failed(_)));
}

#[test]
fn the_login_shell_path_is_read_from_between_the_markers() {
    let output =
        format!("Last login: today\nwelcome\n{PATH_MARKER}/opt/homebrew/bin:/usr/bin{PATH_MARKER}");
    assert_eq!(
        read_login_shell_path(&output).as_deref(),
        Some("/opt/homebrew/bin:/usr/bin")
    );
    assert_eq!(read_login_shell_path("no markers"), None);
    assert_eq!(
        read_login_shell_path(&format!("{PATH_MARKER}{PATH_MARKER}")),
        None
    );
    assert!(login_shell_path_script().contains("\"$PATH\""));
}
