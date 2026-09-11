//! The gate is asked exactly once per tool call, a refusal reaches no host
//! call, and the derived set is the one the direction names.

use std::path::PathBuf;
use std::sync::Mutex;

use tempfile::TempDir;
use writ_mcp::consent::{ClientId, ConsentGate, Decision};
use writ_mcp::tools::{ToolError, ToolHost};

/// A gate that remembers every question it was asked.
struct CountingGate {
    asked: Mutex<Vec<String>>,
    verdict: Decision,
}

impl CountingGate {
    fn new(verdict: Decision) -> Self {
        Self {
            asked: Mutex::new(Vec::new()),
            verdict,
        }
    }
}

impl ConsentGate for CountingGate {
    fn decide(&self, _client: &ClientId, tool: &str) -> Decision {
        self.asked.lock().expect("lock").push(tool.to_string());
        self.verdict
    }
}

struct Fixture {
    _dir: TempDir,
    notes: PathBuf,
    db: PathBuf,
    writ: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    let writ = dir.path().join("writ");
    std::fs::create_dir_all(&notes).expect("notes folder");
    std::fs::create_dir_all(&writ).expect("writ folder");
    Fixture {
        db: dir.path().join("writ.db"),
        _dir: dir,
        notes,
        writ,
    }
}

fn client() -> ClientId {
    ClientId::named("Probe Client")
}

/// Every tool call asks the gate once and no more. A second ask would leave a
/// second line in the activity log for one call.
#[test]
fn each_tool_call_asks_the_gate_once() {
    let fixture = fixture();
    std::fs::write(fixture.notes.join("Launch.md"), "before\n").expect("seed");

    // The gate is boxed into the host, so the count is read through a leaked
    // reference to the same object.
    let gate = Box::leak(Box::new(CountingGate::new(Decision::Allow)));
    let counted: &'static CountingGate = gate;
    struct Forwarding(&'static CountingGate);
    impl ConsentGate for Forwarding {
        fn decide(&self, client: &ClientId, tool: &str) -> Decision {
            self.0.decide(client, tool)
        }
    }

    let host = ToolHost::open(
        &fixture.notes,
        &fixture.db,
        &fixture.writ,
        Box::new(Forwarding(counted)),
    )
    .expect("host");
    let client = client();

    let _ = host.list_notes(&client, None, 10);
    let _ = host.read_note(&client, "Launch.md");
    let _ = host.note_links(&client, "Launch.md");
    let _ = host.folder_tags(&client);
    let _ = host.write_note(&client, "Launch.md", "after\n", None);
    let _ = host.create_note(&client, "Ship it", "body\n");
    let _ = host.rename_note(&client, "Launch.md", "Landed");

    let asked = counted.asked.lock().expect("lock").clone();
    assert_eq!(
        asked,
        vec![
            "list_notes",
            "read_note",
            "note_links",
            "folder_tags",
            "write_note",
            "create_note",
            "rename_note",
        ],
        "one call, one question"
    );
}

/// A refusal ends the call before the host: nothing is read and nothing lands.
#[test]
fn a_refusal_reaches_no_host_call() {
    let fixture = fixture();
    let note = fixture.notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed");

    let host = ToolHost::open(
        &fixture.notes,
        &fixture.db,
        &fixture.writ,
        Box::new(CountingGate::new(Decision::Refuse)),
    )
    .expect("host");
    let client = client();

    assert!(matches!(
        host.read_note(&client, "Launch.md").unwrap_err(),
        ToolError::NotApproved { .. }
    ));
    assert!(matches!(
        host.write_note(&client, "Launch.md", "after\n", None)
            .unwrap_err(),
        ToolError::NotApproved { .. }
    ));
    assert!(matches!(
        host.create_note(&client, "Ship it", "body\n").unwrap_err(),
        ToolError::NotApproved { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "before\n"
    );
    assert!(!fixture.notes.join("Ship it.md").exists());
    assert!(!fixture.db.exists(), "a refused call opened no database");
}

/// An unparseable `expected_hash` on a path the folder does not hold: the
/// hash is read before the path is resolved, so the answer names the hash.
#[test]
fn an_unparseable_hash_outranks_the_path_check() {
    let fixture = fixture();
    let outside = fixture._dir.path().join("elsewhere.md");
    std::fs::write(&outside, "not a note\n").expect("seed outside");

    let host = ToolHost::open(
        &fixture.notes,
        &fixture.db,
        &fixture.writ,
        Box::new(CountingGate::new(Decision::Allow)),
    )
    .expect("host");

    let refusal = host
        .write_note(
            &client(),
            &outside.to_string_lossy(),
            "after\n",
            Some("not-a-hash"),
        )
        .expect_err("both arguments are wrong");

    // Records which of the two messages the client now gets.
    assert!(
        matches!(refusal, ToolError::HashNotUnderstood { .. }),
        "the precedence changed the other way: {refusal:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read back"),
        "not a note\n",
        "nothing outside the folder was written either way"
    );

    // A missing path in the folder, same question.
    let missing = host
        .write_note(&client(), "Gone.md", "after\n", Some("not-a-hash"))
        .expect_err("both arguments are wrong");
    assert!(
        matches!(missing, ToolError::HashNotUnderstood { .. }),
        "{missing:?}"
    );
}
