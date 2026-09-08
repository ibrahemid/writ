//! The server driven over a pipe, the way a client drives it over stdio.
//!
//! Raw JSON-RPC lines rather than an SDK client: this is the wire a client's
//! own implementation meets, so the framing and the method names are part of
//! what is asserted.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use writ_mcp::consent::EnabledReads;
use writ_mcp::tools::{ToolHost, READ_TOOLS};

/// The version this test speaks. rmcp negotiates from its known list.
const PROTOCOL_VERSION: &str = "2025-06-18";

struct Fixture {
    _dir: tempfile::TempDir,
    notes: PathBuf,
    db: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes folder");
    std::fs::write(notes.join("Launch.md"), "# Launch\n\nthe text\n").expect("note");
    std::fs::write(notes.join("Plan.md"), "# Plan\n").expect("note");
    Fixture {
        db: dir.path().join("writ.db"),
        notes,
        _dir: dir,
    }
}

fn host(notes: &Path, db: &Path) -> ToolHost {
    ToolHost::open(notes, db, Box::new(EnabledReads::new(true))).expect("host")
}

/// Sends one JSON-RPC line and reads the next one back.
async fn call<W: AsyncWrite + Unpin, R: tokio::io::AsyncBufRead + Unpin>(
    writer: &mut W,
    reader: &mut tokio::io::Lines<R>,
    request: serde_json::Value,
) -> serde_json::Value {
    send(writer, request).await;
    let line = tokio::time::timeout(std::time::Duration::from_secs(10), reader.next_line())
        .await
        .expect("a reply within ten seconds")
        .expect("read")
        .expect("a reply, not a closed pipe");
    serde_json::from_str(&line).expect("json")
}

/// Sends one JSON-RPC line and expects nothing back.
async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: serde_json::Value) {
    let mut line = serde_json::to_vec(&message).expect("json");
    line.push(b'\n');
    writer.write_all(&line).await.expect("write");
    writer.flush().await.expect("flush");
}

#[tokio::test]
async fn a_client_initialises_lists_the_tools_and_calls_one() {
    let fixture = fixture();
    let host = host(&fixture.notes, &fixture.db);

    let (client, server) = tokio::io::duplex(256 * 1024);
    let served = tokio::spawn(writ_mcp::server::serve_on(host, server));

    let (read_half, mut writer) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half).lines();

    let initialized = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "Test Client", "version": "1.0.0" }
            }
        }),
    )
    .await;

    assert_eq!(initialized["result"]["serverInfo"]["name"], "writ");
    assert!(initialized["result"]["capabilities"]["tools"].is_object());

    send(
        &mut writer,
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;

    let listed = call(
        &mut writer,
        &mut reader,
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
    )
    .await;

    let mut names: Vec<String> = listed["result"]["tools"]
        .as_array()
        .expect("a tool list")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name").to_string())
        .collect();
    names.sort();
    let mut expected: Vec<String> = READ_TOOLS.iter().map(|name| name.to_string()).collect();
    expected.sort();
    assert_eq!(names, expected);

    let called = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "list_notes", "arguments": {} }
        }),
    )
    .await;

    let text = called["result"]["content"][0]["text"]
        .as_str()
        .expect("one text block");
    let notes: Vec<serde_json::Value> = serde_json::from_str(text).expect("json");
    let listed_paths: Vec<&str> = notes
        .iter()
        .map(|note| note["path"].as_str().expect("a path"))
        .collect();
    assert_eq!(listed_paths.len(), 2);
    assert!(listed_paths.iter().any(|path| path.ends_with("Launch.md")));
    assert!(listed_paths.iter().any(|path| path.ends_with("Plan.md")));

    drop(writer);
    drop(reader);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), served).await;
}

#[tokio::test]
async fn a_tool_call_from_a_client_the_user_has_not_turned_the_server_on_for_is_refused() {
    let fixture = fixture();
    let host = ToolHost::open(
        &fixture.notes,
        &fixture.db,
        Box::new(EnabledReads::new(false)),
    )
    .expect("host");

    let (client, server) = tokio::io::duplex(256 * 1024);
    let served = tokio::spawn(writ_mcp::server::serve_on(host, server));

    let (read_half, mut writer) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half).lines();

    call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "Test Client", "version": "1.0.0" }
            }
        }),
    )
    .await;
    send(
        &mut writer,
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;

    let refused = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "read_note", "arguments": { "path": "Launch.md" } }
        }),
    )
    .await;

    let message = refused["error"]["message"]
        .as_str()
        .expect("a refusal message");
    assert!(message.contains("Test Client"), "{message}");
    assert!(!message.contains("the text"), "{message}");

    drop(writer);
    drop(reader);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), served).await;
}
