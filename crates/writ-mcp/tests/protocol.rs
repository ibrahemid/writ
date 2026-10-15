//! The server driven over a pipe, the way a client drives it over stdio.
//!
//! Raw JSON-RPC lines rather than an SDK client: this is the wire a client's
//! own implementation meets, so the framing and the method names are part of
//! what is asserted.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use writ_mcp::consent::{ConfigGate, DenyAll, EnabledReads};
use writ_mcp::tools::{FixedAppFolder, ToolHost, CONTEXT_TOOLS, READ_TOOLS, WRITE_TOOLS};

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
    let writ_dir = db.parent().expect("the database sits in the data folder");
    ToolHost::open(
        notes,
        db,
        writ_dir,
        Box::new(FixedAppFolder(notes.to_path_buf())),
        Box::new(EnabledReads::new(true)),
    )
    .expect("host")
}

/// A host on the gate production runs, over settings that approve the test
/// client for both directions.
fn approved_host(fixture: &Fixture) -> ToolHost {
    let writ_dir = fixture.db.parent().expect("the data folder");
    std::fs::write(
        writ_dir.join("config.toml"),
        "[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"Test Client\"\nread = true\nwrite = true\n",
    )
    .expect("seed the settings file");
    ToolHost::open(
        &fixture.notes,
        &fixture.db,
        writ_dir,
        Box::new(FixedAppFolder(fixture.notes.clone())),
        Box::new(ConfigGate::new(writ_dir)),
    )
    .expect("host")
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
    let mut expected: Vec<String> = READ_TOOLS
        .iter()
        .chain(WRITE_TOOLS.iter())
        .chain(CONTEXT_TOOLS.iter())
        .map(|name| name.to_string())
        .collect();
    expected.sort();
    assert_eq!(names, expected);

    for tool in listed["result"]["tools"].as_array().expect("a tool list") {
        let name = tool["name"].as_str().expect("a name");
        let hints = &tool["annotations"];
        assert_eq!(hints["openWorldHint"], false, "{name}");
        if READ_TOOLS.contains(&name) || CONTEXT_TOOLS.contains(&name) {
            assert_eq!(hints["readOnlyHint"], true, "{name}");
        } else {
            assert_eq!(hints["readOnlyHint"], false, "{name}");
            assert_eq!(
                hints["destructiveHint"],
                ["write_note", "rename_note", "replace_in_note"].contains(&name),
                "{name}: writing, renaming and replacing a passage change what is there; \
                 creating, appending and today's file add to it"
            );
        }
    }

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
    assert!(!text.contains('\n'), "compact, not indented: {text}");
    let page: serde_json::Value = serde_json::from_str(text).expect("json");
    assert_eq!(
        called["result"]["structuredContent"], page,
        "the structured content and the text block carry the same answer"
    );
    let listed_paths: Vec<&str> = page["notes"]
        .as_array()
        .expect("a notes array")
        .iter()
        .map(|note| note["path"].as_str().expect("a path"))
        .collect();
    assert_eq!(listed_paths, vec!["Launch.md", "Plan.md"]);
    assert!(page.get("next_cursor").is_none(), "one page holds both");

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
        fixture.db.parent().expect("the data folder"),
        Box::new(FixedAppFolder(fixture.notes.clone())),
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
    assert_eq!(
        message,
        "Connected programs is off in Writ. Turn it on in Settings, Apps."
    );
    assert!(!message.contains("the text"), "{message}");

    drop(writer);
    drop(reader);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), served).await;
}

#[tokio::test]
async fn a_served_session_writes_the_note_the_client_named() {
    let fixture = fixture();
    let note = fixture.notes.join("Launch.md");
    let host = approved_host(&fixture);

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

    let read = call(
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
    let content: serde_json::Value = serde_json::from_str(
        read["result"]["content"][0]["text"]
            .as_str()
            .expect("one text block"),
    )
    .expect("json");
    let hash = content["hash"].as_str().expect("a hash").to_string();

    let written = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "write_note",
                "arguments": {
                    "path": "Launch.md",
                    "content": "# Launch\n\nthe text a client sent\n",
                    "expected_hash": hash
                }
            }
        }),
    )
    .await;

    let receipt: serde_json::Value = serde_json::from_str(
        written["result"]["content"][0]["text"]
            .as_str()
            .expect("one text block"),
    )
    .expect("json");
    assert_eq!(
        std::fs::read_to_string(&note).expect("read the note back"),
        "# Launch\n\nthe text a client sent\n"
    );
    assert_eq!(
        receipt["bytes"].as_u64(),
        Some("# Launch\n\nthe text a client sent\n".len() as u64)
    );
    assert!(receipt["path"]
        .as_str()
        .expect("a path")
        .ends_with("Launch.md"));

    drop(writer);
    drop(reader);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), served).await;
}

#[tokio::test]
async fn a_second_write_of_the_text_the_client_first_read_is_held_back() {
    let fixture = fixture();
    let note = fixture.notes.join("Launch.md");
    let host = approved_host(&fixture);

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

    let read = call(
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
    let content: serde_json::Value = serde_json::from_str(
        read["result"]["content"][0]["text"]
            .as_str()
            .expect("one text block"),
    )
    .expect("json");
    let hash = content["hash"].as_str().expect("a hash").to_string();

    std::fs::write(&note, "somebody else got there first\n").expect("edit underneath");

    let refused = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "write_note",
                "arguments": {
                    "path": "Launch.md",
                    "content": "what the client sent\n",
                    "expected_hash": hash
                }
            }
        }),
    )
    .await;

    let message = refused["error"]["message"]
        .as_str()
        .expect("a message naming the copy");
    assert!(message.contains("Launch.md"), "{message}");
    assert!(message.contains("conflict"), "{message}");
    assert_eq!(
        std::fs::read_to_string(&note).expect("read the note back"),
        "somebody else got there first\n"
    );

    drop(writer);
    drop(reader);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), served).await;
}

#[tokio::test]
async fn an_unread_note_is_refused_with_how_to_go_on_and_an_overwrite_lands() {
    let fixture = fixture();
    let note = fixture.notes.join("Launch.md");
    let host = approved_host(&fixture);

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

    let listed = call(
        &mut writer,
        &mut reader,
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
    )
    .await;
    let write_tool = listed["result"]["tools"]
        .as_array()
        .expect("a tool list")
        .iter()
        .find(|tool| tool["name"] == "write_note")
        .expect("write_note is listed")
        .clone();
    assert_eq!(
        write_tool["inputSchema"]["properties"]["overwrite"]["type"], "boolean",
        "{write_tool}"
    );
    let required: Vec<&str> = write_tool["inputSchema"]["required"]
        .as_array()
        .map(|names| names.iter().filter_map(|name| name.as_str()).collect())
        .unwrap_or_default();
    assert!(!required.contains(&"overwrite"), "{required:?}");

    let refused = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "write_note",
                "arguments": { "path": "Launch.md", "content": "a blind write\n" }
            }
        }),
    )
    .await;
    let message = refused["error"]["message"]
        .as_str()
        .expect("a refusal, not a result");
    assert!(message.contains("read_note"), "{message}");
    assert_eq!(
        std::fs::read_to_string(&note).expect("read the note back"),
        "# Launch\n\nthe text\n"
    );

    let overwritten = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "write_note",
                "arguments": {
                    "path": "Launch.md",
                    "content": "an overwrite the client asked for\n",
                    "overwrite": true
                }
            }
        }),
    )
    .await;
    assert!(overwritten["result"].is_object(), "{overwritten}");
    assert_eq!(
        std::fs::read_to_string(&note).expect("read the note back"),
        "an overwrite the client asked for\n"
    );

    drop(writer);
    drop(reader);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), served).await;
}

/// Opens a session as `Test Client` and returns the two halves.
async fn session(
    host: ToolHost,
) -> (
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    serde_json::Value,
) {
    let (client, server) = tokio::io::duplex(256 * 1024);
    tokio::spawn(writ_mcp::server::serve_on(host, server));
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
    send(
        &mut writer,
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    (writer, reader, initialized)
}

#[tokio::test]
async fn the_files_and_the_folder_index_are_resources_a_client_can_read() {
    let fixture = fixture();
    let conn = writ_storage::database::connection::open_database(&fixture.db).expect("open");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);
    writ_storage::notes_index::NotesIndexStore::open(&fixture.db)
        .expect("index")
        .reconcile(&fixture.notes, &|| false, &|_| false)
        .expect("reconcile");
    let (mut writer, mut reader, initialized) = session(approved_host(&fixture)).await;
    assert!(initialized["result"]["capabilities"]["resources"].is_object());

    let listed = call(
        &mut writer,
        &mut reader,
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list", "params": {} }),
    )
    .await;
    let uris: Vec<&str> = listed["result"]["resources"]
        .as_array()
        .expect("a resource list")
        .iter()
        .map(|resource| resource["uri"].as_str().expect("a uri"))
        .collect();
    assert_eq!(
        uris,
        vec![
            "writ-folder://index",
            "writ-folder://file/Launch.md",
            "writ-folder://file/Plan.md"
        ]
    );

    let read = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/read",
            "params": { "uri": "writ-folder://file/Launch.md" }
        }),
    )
    .await;
    assert_eq!(
        read["result"]["contents"][0]["text"],
        "# Launch\n\nthe text\n"
    );
    assert_eq!(read["result"]["contents"][0]["mimeType"], "text/markdown");

    let index = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/read",
            "params": { "uri": "writ-folder://index" }
        }),
    )
    .await;
    let text = index["result"]["contents"][0]["text"]
        .as_str()
        .expect("the index as text");
    let parsed: serde_json::Value = serde_json::from_str(text).expect("json");
    let titles: Vec<&str> = parsed["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| file["title"].as_str().expect("a title"))
        .collect();
    assert_eq!(titles, vec!["Launch", "Plan"]);

    let outside = call(
        &mut writer,
        &mut reader,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "resources/read",
            "params": { "uri": "writ-folder://file/..%2Fwrit.db" }
        }),
    )
    .await;
    assert!(outside["error"].is_object(), "{outside}");

    let templates = call(
        &mut writer,
        &mut reader,
        serde_json::json!({ "jsonrpc": "2.0", "id": 6, "method": "resources/templates/list", "params": {} }),
    )
    .await;
    assert_eq!(
        templates["result"]["resourceTemplates"][0]["uriTemplate"],
        "writ-folder://file/{+path}"
    );
}

#[tokio::test]
async fn resources_are_refused_while_the_server_is_off() {
    let fixture = fixture();
    let writ_dir = fixture.db.parent().expect("the data folder").to_path_buf();
    let host = ToolHost::open(
        &fixture.notes,
        &fixture.db,
        &writ_dir,
        Box::new(FixedAppFolder(fixture.notes.clone())),
        Box::new(DenyAll),
    )
    .expect("host");
    let (mut writer, mut reader, _) = session(host).await;
    for (id, method, params) in [
        (2, "resources/list", serde_json::json!({})),
        (
            3,
            "resources/read",
            serde_json::json!({ "uri": "writ-folder://file/Launch.md" }),
        ),
        (
            4,
            "resources/read",
            serde_json::json!({ "uri": "writ-folder://index" }),
        ),
    ] {
        let answer = call(
            &mut writer,
            &mut reader,
            serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
        .await;
        assert!(answer["error"].is_object(), "{method}: {answer}");
        assert!(
            answer["error"]["message"]
                .as_str()
                .expect("a message")
                .contains("Connected programs is off"),
            "{answer}"
        );
    }
}
