//! A registration made before the rename starts the command as `writ`, and the
//! server it reaches answers under the names that registration was approved
//! under (ADR-059 decision 7). Either dialect answers calls and reads under
//! both names.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use jotra_core::mcp_dialect::Dialect;
use jotra_mcp::consent::EnabledReads;
use jotra_mcp::server::JotraServer;
use jotra_mcp::tools::{
    FixedAppFolder, JotraNotReached, JotraOpener, ToolHost, CONTEXT_TOOLS, READ_TOOLS, WRITE_TOOLS,
};

const PROTOCOL_VERSION: &str = "2025-06-18";

type Writer = tokio::io::WriteHalf<tokio::io::DuplexStream>;
type Reader = tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>;

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
    let db = dir.path().join("jotra.db");
    let conn = jotra_storage::database::connection::open_database(&db).expect("open");
    jotra_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);
    jotra_storage::notes_index::NotesIndexStore::open(&db)
        .expect("index")
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile");
    Fixture {
        notes,
        db,
        _dir: dir,
    }
}

#[derive(Clone, Default)]
struct RecordingOpener(Arc<Mutex<Vec<String>>>);

impl JotraOpener for RecordingOpener {
    fn open_link(&self, link: &str) -> Result<(), JotraNotReached> {
        self.0.lock().expect("links").push(link.to_string());
        Ok(())
    }
}

fn host(fixture: &Fixture, opener: &RecordingOpener) -> ToolHost {
    let jotra_dir = fixture.db.parent().expect("the data folder");
    ToolHost::open(
        &fixture.notes,
        &fixture.db,
        jotra_dir,
        Box::new(FixedAppFolder(fixture.notes.clone())),
        Box::new(EnabledReads::new(true)),
    )
    .expect("host")
    .with_opener(Box::new(opener.clone()))
}

async fn call<W: AsyncWrite + Unpin>(
    writer: &mut W,
    reader: &mut Reader,
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

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: serde_json::Value) {
    let mut line = serde_json::to_vec(&message).expect("json");
    line.push(b'\n');
    writer.write_all(&line).await.expect("write");
    writer.flush().await.expect("flush");
}

async fn session(host: ToolHost, dialect: Dialect) -> (Writer, Reader, serde_json::Value) {
    let (client, server) = tokio::io::duplex(256 * 1024);
    tokio::spawn(jotra_mcp::server::serve_on(host, dialect, server));
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

fn every_tool_with_open_named(open_tool: &str) -> Vec<String> {
    let mut names: Vec<String> = READ_TOOLS
        .iter()
        .chain(WRITE_TOOLS.iter())
        .chain(CONTEXT_TOOLS.iter())
        .map(|name| match *name {
            "open_in_jotra" => open_tool.to_string(),
            other => other.to_string(),
        })
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn the_legacy_dialect_names_itself_writ_and_lists_open_in_writ() {
    let fixture = fixture();
    let (mut writer, mut reader, initialized) =
        session(host(&fixture, &RecordingOpener::default()), Dialect::Legacy).await;

    assert_eq!(initialized["result"]["serverInfo"]["name"], "writ");
    let instructions = initialized["result"]["instructions"]
        .as_str()
        .expect("instructions");
    assert!(
        instructions.contains("open_in_writ shows a file"),
        "{instructions}"
    );
    assert!(instructions.contains("Jotra folder"), "{instructions}");

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
    assert_eq!(names, every_tool_with_open_named("open_in_writ"));
}

#[tokio::test]
async fn the_current_dialect_names_itself_jotra_and_lists_open_in_jotra() {
    let fixture = fixture();
    let (mut writer, mut reader, initialized) = session(
        host(&fixture, &RecordingOpener::default()),
        Dialect::Current,
    )
    .await;

    assert_eq!(initialized["result"]["serverInfo"]["name"], "jotra");
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
    assert_eq!(names, every_tool_with_open_named("open_in_jotra"));
}

#[test]
fn the_listed_names_follow_the_dialect() {
    let fixture = fixture();
    let opener = RecordingOpener::default();
    let mut legacy = JotraServer::in_dialect(host(&fixture, &opener), Dialect::Legacy).tool_names();
    legacy.sort();
    assert_eq!(legacy, every_tool_with_open_named("open_in_writ"));

    let mut current = JotraServer::new(host(&fixture, &opener)).tool_names();
    current.sort();
    assert_eq!(current, every_tool_with_open_named("open_in_jotra"));
}

#[tokio::test]
async fn either_open_tool_name_shows_the_file_in_either_dialect() {
    for dialect in Dialect::ALL {
        let fixture = fixture();
        let opener = RecordingOpener::default();
        let (mut writer, mut reader, _) = session(host(&fixture, &opener), dialect).await;

        for (id, name) in [(2, "open_in_writ"), (3, "open_in_jotra")] {
            let answered = call(
                &mut writer,
                &mut reader,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": { "name": name, "arguments": { "path": "Launch.md" } }
                }),
            )
            .await;
            assert!(
                answered["result"]["isError"] != true && answered["error"].is_null(),
                "{dialect:?} {name}: {answered}"
            );
            assert!(
                answered["result"]["structuredContent"]["open_url"]
                    .as_str()
                    .is_some_and(|url| url.starts_with("jotra://")),
                "{dialect:?} {name}: {answered}"
            );
        }
        assert_eq!(opener.0.lock().expect("links").len(), 2, "{dialect:?}");
    }
}

#[tokio::test]
async fn each_dialect_lists_resources_in_its_scheme_and_reads_either() {
    for (dialect, scheme) in [
        (Dialect::Legacy, "writ-folder"),
        (Dialect::Current, "jotra-folder"),
    ] {
        let fixture = fixture();
        let (mut writer, mut reader, _) =
            session(host(&fixture, &RecordingOpener::default()), dialect).await;

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
                format!("{scheme}://index"),
                format!("{scheme}://file/Launch.md")
            ]
        );

        let templates = call(
            &mut writer,
            &mut reader,
            serde_json::json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/templates/list", "params": {} }),
        )
        .await;
        assert_eq!(
            templates["result"]["resourceTemplates"][0]["uriTemplate"],
            format!("{scheme}://file/{{+path}}")
        );

        for (id, uri) in [
            (4, "writ-folder://file/Launch.md"),
            (5, "jotra-folder://file/Launch.md"),
        ] {
            let read = call(
                &mut writer,
                &mut reader,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "resources/read",
                    "params": { "uri": uri }
                }),
            )
            .await;
            assert_eq!(
                read["result"]["contents"][0]["text"], "# Launch\n\nthe text\n",
                "{dialect:?} {uri}: {read}"
            );
        }
        for (id, uri) in [(6, "writ-folder://index"), (7, "jotra-folder://index")] {
            let read = call(
                &mut writer,
                &mut reader,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "resources/read",
                    "params": { "uri": uri }
                }),
            )
            .await;
            assert_eq!(
                read["result"]["contents"][0]["mimeType"], "application/json",
                "{dialect:?} {uri}: {read}"
            );
        }
    }
}
