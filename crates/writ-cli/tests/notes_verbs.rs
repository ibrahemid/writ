//! The note verbs, driven through the built `writ` binary against a fixture
//! notes folder and a fixture database.
//!
//! The database is built the way the app builds it — migrations, then a walk —
//! so what the verbs read is what the app would have written, not rows placed
//! by hand.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index;

/// A fixture instance: a data folder holding `writ.db`, and a notes folder.
struct Fixture {
    _dir: TempDir,
    data: PathBuf,
    notes: PathBuf,
}

impl Fixture {
    /// A notes folder with a resolved link, an unresolved one, an ambiguous
    /// one, an alias, frontmatter and tags. No database yet.
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let data = dir.path().join("data");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&data).expect("create data dir");
        std::fs::create_dir_all(notes.join("a")).expect("create a");
        std::fs::create_dir_all(notes.join("b")).expect("create b");

        write(
            &notes.join("One.md"),
            "---\ntitle: One\nstatus: draft\n---\n\n# Heading\n\n\
             #idea and #draft\n\nSee [[Two]], [[Ghost]] and [[Dup]].\n",
        );
        write(&notes.join("Two.md"), "Back to [[One|the first]]. #idea\n");
        write(&notes.join("a").join("Dup.md"), "one of two\n");
        write(&notes.join("Alone.md"), "nothing points here\n");
        write(&notes.join("b").join("Dup.md"), "two of two\n");

        Self {
            _dir: dir,
            data,
            notes,
        }
    }

    fn db_path(&self) -> PathBuf {
        self.data.join("writ.db")
    }

    /// Builds the database the way the app does, treating the files in
    /// `dataless` as placeholders with no local data.
    fn index(&self, dataless: &HashSet<PathBuf>) {
        let conn = open_database(&self.db_path()).expect("open_database");
        run_migrations(&conn).expect("migrations");
        notes_index::reconcile(&conn, &self.notes, &|| false, &|path: &Path| {
            dataless.contains(path)
        })
        .expect("reconcile");
    }

    fn indexed(&self) -> &Self {
        self.index(&HashSet::new());
        self
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_writ"))
            .args(args)
            .env_remove("WRIT_GUI_BIN")
            .env("WRIT_DATA_DIR", &self.data)
            .env("WRIT_NOTES_DIR", &self.notes)
            .current_dir(&self.notes)
            .output()
            .expect("run writ")
    }
}

fn write(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write note");
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

/// Every record, split into its tab-separated fields.
fn records(output: &Output) -> Vec<Vec<String>> {
    stdout(output)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect()
}

fn json(output: &Output) -> Value {
    serde_json::from_str(&stdout(output)).expect("stdout is one JSON document")
}

fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("no {key} in {row}, which carries {:?}", row))
}

// --------------------------------------------------------------- links

#[test]
fn links_lists_every_link_written_in_a_note() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "One"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let rows = records(&output);
    assert_eq!(rows.len(), 3, "{rows:?}");
    let targets: Vec<&str> = rows.iter().map(|row| row[4].as_str()).collect();
    assert_eq!(targets, vec!["Two", "Ghost", "Dup"]);
    let statuses: Vec<&str> = rows.iter().map(|row| row[3].as_str()).collect();
    assert_eq!(statuses, vec!["resolved", "unresolved", "ambiguous"]);
}

#[test]
fn a_resolved_link_carries_the_note_it_reached() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "One", "--json"]);

    let links = json(&output)["links"].clone();
    let resolved = &links[0];
    assert_eq!(field(resolved, "status"), "resolved");
    assert!(
        field(resolved, "path")
            .as_str()
            .expect("a path")
            .ends_with("Two.md"),
        "{resolved}"
    );
    assert_eq!(
        field(resolved, "candidates").as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(field(resolved, "kind"), "wikilink");
}

#[test]
fn an_unresolved_link_names_no_note_and_offers_no_candidate() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "One", "--json"]);

    let unresolved = json(&output)["links"][1].clone();
    assert_eq!(field(&unresolved, "status"), "unresolved");
    assert!(field(&unresolved, "path").is_null(), "{unresolved}");
    assert_eq!(
        field(&unresolved, "candidates").as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn an_ambiguous_link_names_no_note_and_lists_both_candidates() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "One", "--json"]);

    let ambiguous = json(&output)["links"][2].clone();
    assert_eq!(field(&ambiguous, "status"), "ambiguous");
    assert!(field(&ambiguous, "path").is_null(), "{ambiguous}");
    let candidates = field(&ambiguous, "candidates")
        .as_array()
        .expect("candidates")
        .clone();
    assert_eq!(candidates.len(), 2, "{ambiguous}");
    assert!(candidates
        .iter()
        .all(|path| path.as_str().expect("a path").ends_with("Dup.md")));
}

#[test]
fn the_human_form_of_an_ambiguous_link_shows_both_candidates() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "One"]);

    let ambiguous = &records(&output)[2];
    assert_eq!(ambiguous[3], "ambiguous");
    assert_eq!(ambiguous[5].matches("Dup.md").count(), 2, "{ambiguous:?}");
}

// ------------------------------------------------------------ backlinks

#[test]
fn backlinks_lists_the_note_that_points_here_with_its_sentence() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["backlinks", "One"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let rows = records(&output);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0][0].ends_with("Two.md"), "{rows:?}");
    assert_eq!(rows[0][1], "Two");
    assert_eq!(rows[0][5], "resolved");
    assert_eq!(rows[0][7], "the first", "the alias belongs in the record");
    assert!(rows[0][8].contains("Back to"), "{rows:?}");
}

#[test]
fn backlinks_carries_every_documented_key() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["backlinks", "One", "--json"]);

    let row = json(&output)["backlinks"][0].clone();
    for key in [
        "from_path",
        "from_name",
        "kind",
        "line",
        "col",
        "certainty",
        "target",
        "alias",
        "context",
    ] {
        field(&row, key);
    }
    assert_eq!(field(&row, "alias"), "the first");
    assert_eq!(field(&row, "certainty"), "resolved");
}

#[test]
fn an_ambiguous_link_is_a_backlink_of_both_notes() {
    let fixture = Fixture::new();
    fixture.indexed();

    for note in ["a/Dup.md", "b/Dup.md"] {
        let output = fixture.run(&["backlinks", note, "--json"]);
        let rows = json(&output)["backlinks"].as_array().expect("rows").clone();
        assert_eq!(rows.len(), 1, "{note} has {rows:?}");
        assert_eq!(field(&rows[0], "certainty"), "ambiguous");
    }
}

#[test]
fn a_note_nothing_points_at_lists_nothing_and_still_succeeds() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["backlinks", "Alone"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
}

// ----------------------------------------------------------- properties

#[test]
fn properties_lists_the_frontmatter_in_the_order_it_was_written() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["properties", "One"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let keys: Vec<String> = records(&output)
        .into_iter()
        .map(|row| row[0].clone())
        .collect();
    assert_eq!(keys, vec!["title", "status"]);
}

#[test]
fn a_property_value_crosses_json_as_json() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["properties", "One", "--json"]);

    let rows = json(&output)["properties"]
        .as_array()
        .expect("rows")
        .clone();
    assert_eq!(field(&rows[0], "key"), "title");
    assert_eq!(field(&rows[0], "value"), "One");
}

#[test]
fn a_value_that_is_not_a_string_crosses_json_as_what_it_is() {
    let fixture = Fixture::new();
    write(
        &fixture.notes.join("Shapes.md"),
        "---\ncount: 3\ndone: true\nlist: [a, b]\nblank:\nmeta:\n  a: 1\n  b: 2\n---\n\nbody\n",
    );
    let output = fixture.indexed().run(&["properties", "Shapes", "--json"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let rows = json(&output)["properties"]
        .as_array()
        .expect("rows")
        .clone();
    let value = |key: &str| {
        rows.iter()
            .find(|row| row["key"] == key)
            .unwrap_or_else(|| panic!("no {key} row"))["value"]
            .clone()
    };
    assert_eq!(value("count"), serde_json::json!(3));
    assert_eq!(value("done"), serde_json::json!(true));
    assert_eq!(value("list"), serde_json::json!(["a", "b"]));
    assert_eq!(value("blank"), serde_json::Value::Null);
    // A nested mapping is one the parser does not reduce: it arrives as the
    // text of the block, not as an object.
    assert_eq!(value("meta"), serde_json::json!("  a: 1\n  b: 2"));
}

#[test]
fn a_value_that_is_not_a_string_prints_as_json_on_one_line() {
    let fixture = Fixture::new();
    write(
        &fixture.notes.join("Shapes.md"),
        "---\nlist: [a, b]\nmeta:\n  a: 1\n  b: 2\n---\n\nbody\n",
    );
    let output = fixture.indexed().run(&["properties", "Shapes"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let rows = records(&output);
    assert_eq!(rows.len(), 2, "one record per property: {rows:?}");
    assert_eq!(
        rows[0],
        vec!["list".to_string(), "[\"a\",\"b\"]".to_string()]
    );
    // The block's line break is already escaped by the JSON the value is
    // printed as, so a multi-line property still occupies one record.
    assert_eq!(
        rows[1],
        vec!["meta".to_string(), "\"  a: 1\\n  b: 2\"".to_string()]
    );
}

#[test]
fn a_note_with_no_frontmatter_lists_nothing() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["properties", "Two"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
}

// ----------------------------------------------------------------- tags

#[test]
fn tags_for_a_note_lists_each_tag_with_its_line() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["tags", "One"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let rows = records(&output);
    assert_eq!(
        rows,
        vec![
            vec!["idea".to_string(), "8".to_string()],
            vec!["draft".to_string(), "8".to_string()],
        ]
    );
}

#[test]
fn tags_with_no_note_lists_the_folder_with_a_note_count() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["tags"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(
        records(&output),
        vec![
            vec!["idea".to_string(), "2".to_string()],
            vec!["draft".to_string(), "1".to_string()],
        ]
    );
}

#[test]
fn the_folder_tag_document_names_the_folder_and_no_note() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["tags", "--json"]);

    let document = json(&output);
    assert!(document["note"].is_null(), "{document}");
    assert!(document["notes_folder"]
        .as_str()
        .expect("a folder")
        .ends_with("notes"));
    assert_eq!(field(&document["tags"][0], "tag"), "idea");
    assert_eq!(field(&document["tags"][0], "notes"), 2);
}

#[test]
fn a_note_tag_document_names_the_note_and_the_line() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["tags", "One", "--json"]);

    let document = json(&output);
    assert!(document["note"]
        .as_str()
        .expect("a note")
        .ends_with("One.md"));
    assert_eq!(document["indexed_by"], "content");
    assert_eq!(field(&document["tags"][0], "line"), 8);
}

// ------------------------------------------------------- naming a note

#[test]
fn a_note_is_named_by_a_path_a_name_or_a_name_with_its_extension() {
    let fixture = Fixture::new();
    fixture.indexed();

    for spelling in ["One", "One.md", "./One.md"] {
        let output = fixture.run(&["tags", spelling]);
        assert_eq!(code(&output), 0, "{spelling}: {}", stderr(&output));
        assert_eq!(records(&output).len(), 2, "{spelling}");
    }
}

#[test]
fn a_name_no_note_answers_to_fails_with_a_plain_line() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "Nowhere"]);

    assert_eq!(code(&output), 1);
    assert_eq!(stderr(&output), "writ: no note called Nowhere\n");
    assert_eq!(stdout(&output), "");
}

#[test]
fn a_name_two_notes_answer_to_is_refused_with_both() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "Dup"]);

    assert_eq!(code(&output), 1);
    let said = stderr(&output);
    assert!(said.contains("names more than one note"), "{said}");
    assert_eq!(said.matches("Dup.md").count(), 2, "{said}");
}

// ------------------------------------------------- the index itself

#[test]
fn a_missing_index_is_one_plain_line_and_a_nonzero_exit() {
    let fixture = Fixture::new();
    let output = fixture.run(&["links", "One"]);

    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "");
    let said = stderr(&output);
    assert!(
        said.starts_with("writ: there is no note index at "),
        "{said}"
    );
    assert!(
        !fixture.db_path().exists(),
        "the failed read created the database"
    );
}

#[test]
fn an_index_from_an_older_schema_is_refused() {
    let fixture = Fixture::new();
    fixture.indexed();
    let conn = open_database(&fixture.db_path()).expect("open_database");
    conn.execute(
        "DELETE FROM schema_version WHERE version = (SELECT MAX(version) FROM schema_version)",
        [],
    )
    .expect("drop the last migration record");
    drop(conn);

    let output = fixture.run(&["links", "One"]);
    assert_eq!(code(&output), 1);
    assert!(
        stderr(&output).contains("the note index is at version"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_index_from_a_newer_schema_is_refused() {
    let fixture = Fixture::new();
    fixture.indexed();
    let conn = open_database(&fixture.db_path()).expect("open_database");
    conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (9999, datetime('now'))",
        [],
    )
    .expect("record a newer migration");
    drop(conn);

    let output = fixture.run(&["links", "One"]);
    assert_eq!(code(&output), 1);
    assert!(
        stderr(&output).contains("written by a newer Writ"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_read_leaves_the_database_as_it_found_it() {
    let fixture = Fixture::new();
    fixture.indexed();
    let before = std::fs::read(fixture.db_path()).expect("read db");
    let files_before = data_files(&fixture);

    assert_eq!(code(&fixture.run(&["links", "One"])), 0);
    assert_eq!(code(&fixture.run(&["tags"])), 0);

    assert_eq!(
        std::fs::read(fixture.db_path()).expect("read db"),
        before,
        "a read changed the database"
    );
    // Reading a WAL database makes SQLite create the `-shm` and `-wal`
    // companions if they are not already there. Nothing is written into them.
    for name in data_files(&fixture) {
        assert!(
            files_before.contains(&name) || name == "writ.db-shm" || name == "writ.db-wal",
            "a read left {name} behind"
        );
    }
    let wal = fixture.data.join("writ.db-wal");
    if wal.exists() {
        assert_eq!(
            std::fs::metadata(&wal).expect("wal metadata").len(),
            0,
            "a read wrote frames into the write-ahead log"
        );
    }
}

/// Every file name in the data folder, sorted.
fn data_files(fixture: &Fixture) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(&fixture.data)
        .expect("read data dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_note_the_index_does_not_hold_is_refused() {
    let fixture = Fixture::new();
    fixture.indexed();
    write(&fixture.notes.join("Fresh.md"), "not walked yet\n");

    let output = fixture.run(&["links", "Fresh"]);
    assert_eq!(code(&output), 1);
    assert!(
        stderr(&output).contains("does not hold"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_note_held_by_name_alone_says_so_rather_than_reading_as_empty() {
    let fixture = Fixture::new();
    let away = fixture.notes.join("Away.md");
    write(&away, "#idea\n\n[[Two]]\n");
    fixture.index(&HashSet::from([away]));

    let output = fixture.run(&["links", "Away"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "writ: this note has no data on this machine, so nothing was read out of it\n"
    );
}

#[test]
fn a_note_held_by_name_alone_says_so_in_the_document_too() {
    let fixture = Fixture::new();
    let away = fixture.notes.join("Away.md");
    write(&away, "#idea\n");
    fixture.index(&HashSet::from([away]));

    let output = fixture.run(&["tags", "Away", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(json(&output)["indexed_by"], "name");
    assert_eq!(stderr(&output), "", "the document already says it");
}

// -------------------------------------------------------- writing verbs

#[test]
fn new_creates_a_note_in_the_notes_folder_and_prints_its_path() {
    let fixture = Fixture::new();
    let output = fixture.run(&["new", "Ideas"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let path = PathBuf::from(stdout(&output).trim());
    assert!(path.is_file(), "{path:?} is not a file");
    assert_eq!(
        path.canonicalize().expect("canonicalize"),
        fixture
            .notes
            .join("Ideas.md")
            .canonicalize()
            .expect("canonicalize")
    );
}

#[test]
fn new_needs_no_index() {
    let fixture = Fixture::new();
    let output = fixture.run(&["new", "Ideas"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(!fixture.db_path().exists());
}

#[test]
fn new_dedupes_against_what_the_folder_already_holds() {
    let fixture = Fixture::new();
    assert_eq!(
        PathBuf::from(stdout(&fixture.run(&["new", "One"])).trim()).file_name(),
        fixture.notes.join("One 2.md").file_name()
    );
}

#[test]
fn new_with_no_name_dates_the_note() {
    let fixture = Fixture::new();
    let output = fixture.run(&["new"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let stem = PathBuf::from(stdout(&output).trim())
        .file_stem()
        .expect("a stem")
        .to_string_lossy()
        .into_owned();
    assert_eq!(stem.len(), 10, "{stem} is not a date");
    assert_eq!(stem.matches('-').count(), 2, "{stem} is not a date");
}

#[test]
fn rename_moves_the_note_inside_its_folder() {
    let fixture = Fixture::new();
    let output = fixture.run(&["rename", "a/Dup.md", "Renamed"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let renamed = PathBuf::from(stdout(&output).trim());
    assert!(renamed.is_file(), "{renamed:?} is not a file");
    assert_eq!(
        renamed.canonicalize().expect("canonicalize"),
        fixture
            .notes
            .join("a")
            .join("Renamed.md")
            .canonicalize()
            .expect("canonicalize")
    );
    assert!(!fixture.notes.join("a").join("Dup.md").exists());
}

#[test]
fn the_rename_document_carries_both_paths() {
    let fixture = Fixture::new();
    let output = fixture.run(&["rename", "One", "Renamed", "--json"]);

    let document = json(&output);
    assert!(document["note"]
        .as_str()
        .expect("a note")
        .ends_with("Renamed.md"));
    assert!(document["previous_path"]
        .as_str()
        .expect("a previous path")
        .ends_with("One.md"));
}

#[test]
fn rename_onto_a_name_the_folder_already_holds_is_refused() {
    let fixture = Fixture::new();
    let output = fixture.run(&["rename", "One", "Two"]);

    assert_eq!(code(&output), 1);
    assert!(
        stderr(&output).starts_with("writ: cannot rename "),
        "{}",
        stderr(&output)
    );
    assert!(
        fixture.notes.join("One.md").exists(),
        "the note was moved anyway"
    );
}

#[test]
fn renaming_a_note_that_is_not_there_is_refused() {
    let fixture = Fixture::new();
    let output = fixture.run(&["rename", "Nowhere", "Somewhere"]);

    assert_eq!(code(&output), 1);
    assert_eq!(stderr(&output), "writ: no note called Nowhere\n");
}

#[test]
fn trash_takes_the_note_off_disk_and_prints_where_it_was() {
    let fixture = Fixture::new();
    let output = fixture.run(&["trash", "Two"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(
        PathBuf::from(stdout(&output).trim()).file_name(),
        fixture.notes.join("Two.md").file_name()
    );
    assert!(!fixture.notes.join("Two.md").exists());
}

#[test]
fn trashing_a_note_that_is_not_there_is_refused() {
    let fixture = Fixture::new();
    let output = fixture.run(&["trash", "Nowhere"]);

    assert_eq!(code(&output), 1);
    assert_eq!(stderr(&output), "writ: no note called Nowhere\n");
}

// ----------------------------------------------------------------- usage

#[test]
fn a_verb_with_no_note_prints_usage_and_exits_two() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links"]);

    assert_eq!(code(&output), 2);
    let said = stderr(&output);
    assert!(said.contains("writ links needs a note"), "{said}");
    assert!(said.contains("Usage: writ <verb>"), "{said}");
}

#[test]
fn a_flag_a_verb_does_not_take_prints_usage_and_exits_two() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["tags", "--yaml"]);

    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("does not take --yaml"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn too_many_arguments_prints_usage_and_exits_two() {
    let fixture = Fixture::new();
    let output = fixture.indexed().run(&["links", "One", "Two"]);

    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("more arguments than it takes"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_usage_text_lists_every_verb() {
    let fixture = Fixture::new();
    let said = stderr(&fixture.run(&["links"]));
    for verb in [
        "links",
        "backlinks",
        "properties",
        "tags",
        "new",
        "rename",
        "trash",
    ] {
        assert!(said.contains(verb), "usage does not mention {verb}");
    }
}

/// The file-opening path is unchanged: a first argument that is not a verb name
/// still reaches it. Off macOS, where the app is resolved by binary rather than
/// by bundle id, a stub stands in for it.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_path_argument_still_opens_a_file() {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_writ"))
        .arg("One.md")
        .env("WRIT_GUI_BIN", "/bin/true")
        .env("WRIT_DATA_DIR", &fixture.data)
        .env("WRIT_NOTES_DIR", &fixture.notes)
        .current_dir(&fixture.notes)
        .output()
        .expect("run writ");

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "", "the file path was read as a verb");
}
