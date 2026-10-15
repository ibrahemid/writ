//! The verbs that run on the note host, and the opens that go to the running
//! app, driven through the built `writ` binary.
//!
//! No test here starts the app. An open that would go through the operating
//! system is never asked for; an open carrying a line or a wait finds the data
//! folder held by the test, which plays the running app: it takes the request
//! from the folder and answers the wait.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;
use writ_core::instance::OpenRequest;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::instance::{acquire, release_wait, take_requests};
use writ_storage::note_history::NoteHistoryStore;
use writ_storage::notes_index;

/// A data folder whose config names the notes folder, so the notes folder is
/// the app's and a write keeps a version in the app's store.
struct Fixture {
    _dir: TempDir,
    data: PathBuf,
    notes: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let data = dir.path().join("data");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&data).expect("data");
        std::fs::create_dir_all(notes.join("projects")).expect("notes");
        std::fs::write(
            data.join("config.toml"),
            format!("[notes]\nroot = {:?}\n", notes.to_string_lossy()),
        )
        .expect("config");
        std::fs::write(notes.join("Plan.txt"), "launch the site\nthen the app\n").expect("plan");
        std::fs::write(
            notes.join("projects").join("Ideas.md"),
            "# Ideas\nlaunch party\n",
        )
        .expect("ideas");
        Self {
            _dir: dir,
            data,
            notes,
        }
    }

    fn indexed(self) -> Self {
        let conn = open_database(&self.data.join("writ.db")).expect("db");
        run_migrations(&conn).expect("migrations");
        notes_index::reconcile(&conn, &self.notes, &|| false, &|_: &Path| false)
            .expect("reconcile");
        self
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_writ"));
        command
            .args(args)
            .env_remove("WRIT_GUI_BIN")
            .env("WRIT_DATA_DIR", &self.data)
            .env("WRIT_NOTES_DIR", &self.notes)
            .current_dir(&self.notes);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("run writ")
    }

    fn piped(&self, args: &[&str], input: &str) -> Output {
        let mut command = self.command(args);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("run writ");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("wait for writ")
    }

    /// The folder as the host resolves it, which is how every path it hands
    /// back is spelled.
    fn resolved(&self) -> PathBuf {
        std::fs::canonicalize(&self.notes).expect("canonical notes")
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is text")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is text")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("an exit code")
}

fn json(output: &Output) -> Value {
    serde_json::from_str(&stdout(output))
        .unwrap_or_else(|_| panic!("one JSON document: {}", stdout(output)))
}

/// Plays the running app: takes the first request that arrives, answers its
/// wait after `answer_after`, and hands it back.
fn running_app(
    data: &Path,
    answer_after: Option<Duration>,
) -> std::thread::JoinHandle<OpenRequest> {
    let data = data.to_path_buf();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(request) = take_requests(&data).into_iter().next() {
                if let (Some(after), Some(token)) = (answer_after, request.wait_token()) {
                    std::thread::sleep(after);
                    release_wait(&data, token);
                }
                return request;
            }
            assert!(Instant::now() < deadline, "no request arrived");
            std::thread::sleep(Duration::from_millis(5));
        }
    })
}

// ------------------------------------------------------------------ search

#[test]
fn search_lists_the_files_that_match_with_their_lines() {
    let fixture = Fixture::new().indexed();
    let output = fixture.run(&["search", "launch", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document = json(&output);
    assert_eq!(document["query"], "launch");
    let hits = document["hits"].as_array().expect("hits");
    let paths: Vec<&str> = hits
        .iter()
        .map(|hit| hit["path"].as_str().expect("path"))
        .collect();
    assert!(
        paths.iter().any(|path| path.ends_with("Plan.txt")),
        "{paths:?}"
    );
    assert!(
        paths.iter().any(|path| path.ends_with("Ideas.md")),
        "{paths:?}"
    );

    let plain = fixture.run(&["search", "launch", "--limit", "1"]);
    assert_eq!(code(&plain), 0);
    assert_eq!(stdout(&plain).lines().count(), 1);
    assert_eq!(
        stdout(&plain).lines().next().unwrap().split('\t').count(),
        3
    );
}

#[test]
fn search_with_no_index_says_why_and_exits_one() {
    let fixture = Fixture::new();
    let output = fixture.run(&["search", "launch", "--json"]);
    assert_eq!(code(&output), 1);
    assert!(
        stderr(&output).contains("there is no index"),
        "{}",
        stderr(&output)
    );
    assert!(stdout(&output).is_empty());
}

// --------------------------------------------------------------------- cat

#[test]
fn cat_prints_the_text_as_written() {
    let fixture = Fixture::new();
    let output = fixture.run(&["cat", "Plan"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "launch the site\nthen the app\n");

    let output = fixture.run(&["cat", "Plan.txt", "--json"]);
    let document = json(&output);
    assert_eq!(document["text"], "launch the site\nthen the app\n");
    assert_eq!(document["bytes"], 29);
    assert_eq!(document["hash"].as_str().expect("hash").len(), 64);
    assert!(document["note"]
        .as_str()
        .expect("note")
        .ends_with("Plan.txt"));
}

#[test]
fn cat_refuses_a_file_outside_the_folder_and_a_missing_one() {
    let fixture = Fixture::new();
    let outside = fixture.data.join("config.toml");
    let output = fixture.run(&["cat", &outside.to_string_lossy()]);
    assert_eq!(code(&output), 1);
    assert!(
        stderr(&output).contains("is not in your folder"),
        "{}",
        stderr(&output)
    );

    let output = fixture.run(&["cat", "Nowhere", "--json"]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).is_empty());
}

// ---------------------------------------------------------------------- ls

#[test]
fn ls_lists_every_file_or_one_folder_in_path_order() {
    let fixture = Fixture::new();
    let output = fixture.run(&["ls", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document = json(&output);
    let notes = document["notes"].as_array().expect("notes");
    let names: Vec<&str> = notes
        .iter()
        .map(|note| note["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, vec!["Plan.txt", "Ideas"]);
    assert_eq!(notes[0]["bytes"], 29);

    let output = fixture.run(&["ls", "projects/"]);
    assert_eq!(code(&output), 0);
    let text = stdout(&output);
    let records: Vec<&str> = text.lines().collect();
    assert_eq!(records.len(), 1);
    assert!(records[0].contains("Ideas.md\tIdeas\t"), "{}", records[0]);
}

// ------------------------------------------------------------------ append

#[test]
fn append_adds_piped_text_through_the_guard_and_keeps_a_version() {
    let fixture = Fixture::new();
    let output = fixture.piped(&["append", "Plan.txt", "--json"], "hi\n");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document = json(&output);
    assert_eq!(document["changed"], true);
    assert_eq!(
        std::fs::read_to_string(fixture.notes.join("Plan.txt")).expect("read"),
        "launch the site\nthen the app\nhi\n"
    );

    let store = NoteHistoryStore::open(&fixture.data).expect("store");
    store.set_notes_root(fixture.resolved());
    let key = store
        .key_for(&fixture.resolved().join("Plan.txt"))
        .expect("key");
    let kept: Vec<Vec<u8>> = store
        .versions(&key)
        .expect("versions")
        .into_iter()
        .map(|entry| store.content(entry.id).expect("content"))
        .collect();
    assert!(
        kept.contains(&b"launch the site\nthen the app\n".to_vec()),
        "{kept:?}"
    );
}

#[test]
fn append_to_a_name_nothing_has_creates_it() {
    let fixture = Fixture::new();
    let output = fixture.piped(&["append", "Log"], "first line\n");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).trim_end().ends_with("Log.txt"),
        "{}",
        stdout(&output)
    );
    assert_eq!(
        std::fs::read_to_string(fixture.notes.join("Log.txt")).expect("read"),
        "first line\n"
    );
}

#[test]
fn append_with_nothing_piped_is_a_usage_error() {
    let fixture = Fixture::new();
    let output = fixture.run(&["append", "Plan.txt"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("nothing is piped"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        std::fs::read_to_string(fixture.notes.join("Plan.txt")).expect("read"),
        "launch the site\nthen the app\n"
    );
}

// -------------------------------------------------------------------- today

#[test]
fn today_mints_the_days_file_and_appends_to_it_without_opening() {
    let fixture = Fixture::new();
    let output = fixture.run(&["today", "--no-open", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document = json(&output);
    assert_eq!(document["created"], true);
    let note = PathBuf::from(document["note"].as_str().expect("note"));
    let name = note
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert_eq!(name, format!("{today}.txt"));

    let output = fixture.piped(&["today", "--append", "--no-open", "--json"], "- milk\n");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document = json(&output);
    assert_eq!(document["created"], false);
    assert_eq!(document["changed"], true);
    assert_eq!(std::fs::read_to_string(&note).expect("read"), "- milk\n");
}

// --------------------------------------------------------------------- new

#[test]
fn new_holds_piped_text() {
    let fixture = Fixture::new();
    let output = fixture.piped(&["new", "Groceries", "--json"], "eggs\n");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let note = PathBuf::from(json(&output)["note"].as_str().expect("note"));
    assert_eq!(std::fs::read_to_string(note).expect("read"), "eggs\n");
}

// -------------------------------------------------------- opening in the app

#[test]
fn open_by_name_at_a_line_hands_the_running_app_the_line() {
    let fixture = Fixture::new();
    let _app = acquire(&fixture.data).expect("the test plays the running app");
    let app = running_app(&fixture.data, None);
    let output = fixture.run(&["open", "Plan:2", "--json"]);
    let request = app.join().expect("app");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document = json(&output);
    assert_eq!(document["line"], 2);
    let note = document["note"].as_str().expect("note").to_string();
    assert!(note.ends_with("Plan.txt"), "{note}");
    assert_eq!(request.paths, vec![note.clone()]);
    assert_eq!(request.lines.get(&note), Some(&2));
    assert_eq!(request.wait, None);
}

#[test]
fn open_of_a_name_nothing_has_exits_one_and_opens_nothing() {
    let fixture = Fixture::new();
    let output = fixture.run(&["open", "Nowhere:3", "--json"]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).is_empty());
    assert!(take_requests(&fixture.data).is_empty());
}

#[test]
fn a_path_ending_in_a_line_opens_there() {
    let fixture = Fixture::new();
    let _app = acquire(&fixture.data).expect("the test plays the running app");
    let app = running_app(&fixture.data, None);
    let output = fixture.run(&["Plan.txt:2"]);
    let request = app.join().expect("app");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    // Joined to the working folder as the command reads it, which is
    // resolved on macOS (`/private/var` for `/var`).
    let expected = fixture
        .resolved()
        .join("Plan.txt")
        .to_string_lossy()
        .into_owned();
    assert_eq!(request.paths, vec![expected.clone()]);
    assert_eq!(request.lines.get(&expected), Some(&2));
}

#[test]
fn wait_returns_when_the_tab_closes() {
    let fixture = Fixture::new();
    std::fs::write(fixture.notes.join("COMMIT_EDITMSG"), "\n").expect("message");
    let _app = acquire(&fixture.data).expect("the test plays the running app");
    let app = running_app(&fixture.data, Some(Duration::from_millis(300)));
    let started = Instant::now();
    let output = fixture.run(&["--wait", "COMMIT_EDITMSG"]);
    let request = app.join().expect("app");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(request.wait_token().is_some());
    assert_eq!(
        request.paths,
        vec![fixture
            .resolved()
            .join("COMMIT_EDITMSG")
            .to_string_lossy()
            .into_owned()]
    );
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "returned before the tab closed"
    );
}

#[test]
fn wait_returns_when_the_app_ends() {
    let fixture = Fixture::new();
    std::fs::write(fixture.notes.join("COMMIT_EDITMSG"), "\n").expect("message");
    let held = acquire(&fixture.data).expect("the test plays the running app");
    let app = running_app(&fixture.data, None);
    let child = fixture
        .command(&["--wait", "COMMIT_EDITMSG"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run writ");
    app.join().expect("app");
    std::thread::sleep(Duration::from_millis(200));
    drop(held);
    let output = child.wait_with_output().expect("wait");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
}

#[test]
fn wait_on_a_folder_is_a_usage_error() {
    let fixture = Fixture::new();
    let output = fixture.run(&["--wait", "projects"]);
    assert_eq!(code(&output), 2);
}

#[test]
fn no_open_saves_piped_text_and_prints_where() {
    let fixture = Fixture::new();
    let output = fixture.piped(&["--no-open", "--title", "build log"], "ok\n");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let path = PathBuf::from(stdout(&output).trim_end());
    assert_eq!(path.file_name().expect("name"), "build log.txt");
    assert_eq!(std::fs::read_to_string(path).expect("read"), "ok\n");

    let given = fixture.run(&["--no-open", "Plan.txt"]);
    assert_eq!(code(&given), 2);
}
