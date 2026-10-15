//! `get_context` through the built `writ mcp` (ADR-053), with the test playing
//! the running app: it holds the data folder, takes the ask from
//! `open-requests/` and leaves the answer in `context-answers/`.
//!
//! The server is checked for sockets before and after the call: the channel is
//! files in the data folder, so the process holds none at rest.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use writ_core::open_context::{OpenContext, Selected, TextPoint};
use writ_storage::instance::{acquire, take_requests};
use writ_storage::open_context::write_answer;

const CLIENT: &str = "Smoke Client";

struct Fixture {
    _dir: TempDir,
    data: PathBuf,
    notes: PathBuf,
}

fn fixture(context: bool) -> Fixture {
    let dir = TempDir::new().expect("tempdir");
    let data = dir.path().join("data");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&data).expect("data");
    std::fs::create_dir_all(&notes).expect("notes");
    std::fs::write(
        data.join("config.toml"),
        format!(
            "[notes]\nroot = {:?}\n\n[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"{CLIENT}\"\nread = true\ncontext = {context}\n",
            notes.to_string_lossy()
        ),
    )
    .expect("config");
    std::fs::write(notes.join("Plan.md"), "# Plan\nlaunch the site\n").expect("plan");
    Fixture {
        _dir: dir,
        data,
        notes,
    }
}

/// Plays the running app until `stop`: answers every context ask with the
/// plan and its second line selected.
fn answer_asks(
    data: PathBuf,
    plan: PathBuf,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<usize> {
    std::thread::spawn(move || {
        let mut answered = 0;
        while !stop.load(Ordering::SeqCst) {
            for request in take_requests(&data) {
                let Some(ask) = request.context else { continue };
                assert_eq!(ask.program, CLIENT);
                let context = OpenContext {
                    path: Some(plan.to_string_lossy().into_owned()),
                    selections: vec![Selected {
                        text: "launch the site".to_string(),
                        from: TextPoint { line: 2, column: 1 },
                        to: TextPoint {
                            line: 2,
                            column: 16,
                        },
                    }],
                    ..OpenContext::default()
                };
                write_answer(&data, &ask.token, &context).expect("answer");
                answered += 1;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        answered
    })
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn start(fixture: &Fixture) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_writ"))
            .arg("mcp")
            .env_remove("WRIT_GUI_BIN")
            .env("WRIT_DATA_DIR", &fixture.data)
            .env("WRIT_NOTES_DIR", &fixture.notes)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start writ mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut server = Self {
            child,
            stdin,
            stdout,
        };
        let initialized = server.call(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": CLIENT, "version": "1.0.0" }
            }
        }));
        assert_eq!(initialized["result"]["serverInfo"]["name"], "writ");
        server.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        server
    }

    fn send(&mut self, message: Value) {
        let mut line = serde_json::to_vec(&message).expect("json");
        line.push(b'\n');
        self.stdin.write_all(&line).expect("write");
        self.stdin.flush().expect("flush");
    }

    fn call(&mut self, message: Value) -> Value {
        self.send(message);
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("a reply");
        serde_json::from_str(&line).expect("json")
    }

    fn get_context(&mut self, id: u64) -> Value {
        self.call(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": "get_context", "arguments": {} }
        }))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The sockets process `pid` holds, as the platform lists them.
#[cfg(target_os = "macos")]
fn sockets_of(pid: u32) -> Vec<String> {
    let mut held = Vec::new();
    for kind in ["-i", "-U"] {
        let output = Command::new("/usr/sbin/lsof")
            .args(["-nP", "-a", "-p", &pid.to_string(), kind])
            .output()
            .expect("lsof");
        held.extend(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .skip(1)
                .map(str::to_string),
        );
    }
    held
}

#[cfg(target_os = "linux")]
fn sockets_of(pid: u32) -> Vec<String> {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .expect("the process's descriptors")
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .map(|target| target.to_string_lossy().into_owned())
        .filter(|target| target.starts_with("socket:"))
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn sockets_of(_pid: u32) -> Vec<String> {
    Vec::new()
}

#[test]
fn an_allowed_program_reads_the_open_file_through_the_data_folder_and_no_socket() {
    let fixture = fixture(true);
    let _app = acquire(&fixture.data).expect("the test plays the running app");
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_asks(
        fixture.data.clone(),
        fixture.notes.join("Plan.md"),
        Arc::clone(&stop),
    );
    let mut server = Server::start(&fixture);
    let pid = server.child.id();

    assert_eq!(
        sockets_of(pid),
        Vec::<String>::new(),
        "at rest, before a call"
    );

    let reply = server.get_context(2);
    let answer = &reply["result"]["structuredContent"];
    assert_eq!(answer["path"], "Plan.md", "{reply}");
    assert_eq!(answer["in_folder"], true);
    assert_eq!(answer["selections"][0]["text"], "launch the site");
    assert_eq!(answer["open_url"], "writ://open?path=Plan.md");

    assert_eq!(
        sockets_of(pid),
        Vec::<String>::new(),
        "at rest, after a call"
    );

    stop.store(true, Ordering::SeqCst);
    assert_eq!(answering.join().expect("the app"), 1);
    let log = std::fs::read_to_string(fixture.data.join("activity.jsonl")).expect("the log");
    assert!(
        log.contains("get_context") && log.contains("Plan.md"),
        "{log}"
    );
    assert!(!log.contains("launch the site"), "{log}");
    assert_eq!(
        std::fs::read_dir(fixture.data.join("context-answers"))
            .expect("the answers folder")
            .count(),
        0
    );
}

#[test]
fn a_program_without_the_open_file_grant_is_refused_and_the_app_is_not_asked() {
    let fixture = fixture(false);
    let _app = acquire(&fixture.data).expect("the test plays the running app");
    let mut server = Server::start(&fixture);

    let reply = server.get_context(2);

    let message = reply["error"]["message"].as_str().expect("a refusal");
    assert!(
        message.contains("not allowed to see the file open in Writ"),
        "{message}"
    );
    assert!(take_requests(&fixture.data).is_empty());
    assert!(!Path::new(&fixture.data.join("context-answers")).exists());
}
