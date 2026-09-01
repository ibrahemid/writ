//! The note verbs: `links`, `backlinks`, `properties`, `tags`, `new`, `rename`
//! and `trash`.
//!
//! Every verb answers from the same two places the app answers from — the notes
//! folder and the note index inside `writ.db` — so the command line and the
//! window never disagree. The index is opened read-only: this process must not
//! create a database, run a migration or change a row, and a stale or absent
//! index is reported rather than repaired.
//!
//! Link resolution is `writ_core::notes::links` through `writ-storage`. Nothing
//! here re-implements it.
//!
//! None of the verbs opens a window. They print records and exit, so they can
//! be piped; `writ <file>` is still how a note is opened.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use writ_core::notes::links::Resolution;
use writ_core::notes::{dedupe_file_name, note_display_name, note_file_stem};
use writ_storage::database::migrations::binary_schema_version;
use writ_storage::note_ops;
use writ_storage::notes_index::{self, IndexedBy, NotesIndexStore};

/// Extension a new note is created with.
const NOTE_EXTENSION: &str = "md";

/// Everything read successfully.
pub const EXIT_OK: i32 = 0;
/// The note, the index or the operation was not there. See the module doc of
/// each verb for which failures land here.
pub const EXIT_FAILED: i32 = 1;
/// The command line could not be read as a verb.
pub const EXIT_USAGE: i32 = 2;

/// The verbs, in the order the usage text lists them.
const VERB_NAMES: &[&str] = &[
    "links",
    "backlinks",
    "properties",
    "tags",
    "new",
    "rename",
    "trash",
];

/// One parsed verb invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verb {
    /// Links written in a note.
    Links { note: String, json: bool },
    /// Links in other notes that point at a note.
    Backlinks { note: String, json: bool },
    /// A note's frontmatter properties.
    Properties { note: String, json: bool },
    /// A note's tags, or every tag in the notes folder.
    Tags { note: Option<String>, json: bool },
    /// Create a note in the notes folder.
    New { name: Option<String>, json: bool },
    /// Rename a note in place.
    Rename {
        note: String,
        new_name: String,
        json: bool,
    },
    /// Move a note to the operating system's trash.
    Trash { note: String, json: bool },
}

/// Why a command line could not be read as a verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    /// A flag the verb does not take.
    UnknownFlag { verb: String, flag: String },
    /// An argument the verb needs was not given.
    MissingArgument { verb: String, what: String },
    /// More arguments than the verb takes.
    TooManyArguments { verb: String },
    /// An argument that is not valid text.
    NotText { verb: String },
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageError::UnknownFlag { verb, flag } => {
                write!(f, "writ {verb} does not take {flag}")
            }
            UsageError::MissingArgument { verb, what } => {
                write!(f, "writ {verb} needs {what}")
            }
            UsageError::TooManyArguments { verb } => {
                write!(f, "writ {verb} was given more arguments than it takes")
            }
            UsageError::NotText { verb } => {
                write!(f, "writ {verb} was given an argument that is not text")
            }
        }
    }
}

/// The usage text, listing every verb and what it takes.
pub fn usage() -> String {
    [
        "Usage: writ <verb> [arguments]",
        "",
        "  writ links <note> [--json]        links written in a note",
        "  writ backlinks <note> [--json]    links in other notes pointing at it",
        "  writ properties <note> [--json]   the note's frontmatter properties",
        "  writ tags [<note>] [--json]       a note's tags, or every tag in the notes folder",
        "  writ new [<name>] [--json]        create a note in the notes folder",
        "  writ rename <note> <new-name>     rename a note, keeping its folder",
        "  writ trash <note>                 move a note to the trash",
        "",
        "A <note> is a path, or the name of a note in the notes folder.",
        "Exit codes: 0 read, 1 nothing to read or the operation failed, 2 bad arguments.",
        "",
        "Open a file instead: writ <path>",
    ]
    .join("\n")
}

/// Reads a verb off the front of `args`, or `None` when the first argument does
/// not name one and the invocation belongs to the file-opening path.
pub fn parse(args: &[OsString]) -> Option<Result<Verb, UsageError>> {
    let name = args.first()?.to_str()?;
    if !VERB_NAMES.contains(&name) {
        return None;
    }
    Some(parse_verb(name, &args[1..]))
}

/// Splits `--json` out of the remaining arguments, refusing any other flag.
fn split_flags(verb: &str, rest: &[OsString]) -> Result<(Vec<String>, bool), UsageError> {
    let mut positional = Vec::new();
    let mut json = false;
    for arg in rest {
        let Some(text) = arg.to_str() else {
            return Err(UsageError::NotText {
                verb: verb.to_string(),
            });
        };
        if text == "--json" {
            json = true;
        } else if text.starts_with("--") {
            return Err(UsageError::UnknownFlag {
                verb: verb.to_string(),
                flag: text.to_string(),
            });
        } else {
            positional.push(text.to_string());
        }
    }
    Ok((positional, json))
}

fn parse_verb(name: &str, rest: &[OsString]) -> Result<Verb, UsageError> {
    let (mut positional, json) = split_flags(name, rest)?;
    let too_many = || UsageError::TooManyArguments {
        verb: name.to_string(),
    };
    let missing = |what: &str| UsageError::MissingArgument {
        verb: name.to_string(),
        what: what.to_string(),
    };

    match name {
        "links" | "backlinks" | "properties" | "trash" => {
            if positional.len() > 1 {
                return Err(too_many());
            }
            let note = positional.pop().ok_or_else(|| missing("a note"))?;
            Ok(match name {
                "links" => Verb::Links { note, json },
                "backlinks" => Verb::Backlinks { note, json },
                "properties" => Verb::Properties { note, json },
                _ => Verb::Trash { note, json },
            })
        }
        "tags" => {
            if positional.len() > 1 {
                return Err(too_many());
            }
            Ok(Verb::Tags {
                note: positional.pop(),
                json,
            })
        }
        "new" => {
            if positional.len() > 1 {
                return Err(too_many());
            }
            Ok(Verb::New {
                name: positional.pop(),
                json,
            })
        }
        "rename" => {
            if positional.len() > 2 {
                return Err(too_many());
            }
            let mut args = positional.into_iter();
            let note = args.next().ok_or_else(|| missing("a note"))?;
            let new_name = args.next().ok_or_else(|| missing("a new name"))?;
            Ok(Verb::Rename {
                note,
                new_name,
                json,
            })
        }
        _ => Err(missing("a verb")),
    }
}

/// Where a verb reads and writes: resolved once, so a test can point the whole
/// surface at a fixture folder.
#[derive(Debug, Clone)]
pub struct Context {
    /// The directory a relative note argument is joined to.
    pub cwd: PathBuf,
    /// The notes folder, resolved the way the app resolves it.
    pub notes_dir: PathBuf,
    /// `writ.db`, holding the note index.
    pub db_path: PathBuf,
    /// The moment a note with no name of its own is named for.
    pub now: chrono::DateTime<chrono::Utc>,
}

/// What a verb produced: the two streams and the exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    fn ok(stdout: String) -> Self {
        Self {
            code: EXIT_OK,
            stdout,
            stderr: String::new(),
        }
    }

    fn failed(message: String) -> Self {
        Self {
            code: EXIT_FAILED,
            stdout: String::new(),
            stderr: format!("writ: {message}\n"),
        }
    }

    fn with_note(mut self, note: String) -> Self {
        self.stderr.push_str(&format!("writ: {note}\n"));
        self
    }
}

/// Runs a parsed verb against `ctx`.
pub fn run(verb: Verb, ctx: &Context) -> Outcome {
    match verb {
        Verb::Links { note, json } => read_verb(&note, json, ctx, links_document),
        Verb::Backlinks { note, json } => read_verb(&note, json, ctx, backlinks_document),
        Verb::Properties { note, json } => read_verb(&note, json, ctx, properties_document),
        Verb::Tags {
            note: Some(note),
            json,
        } => read_verb(&note, json, ctx, note_tags_document),
        Verb::Tags { note: None, json } => folder_tags(json, ctx),
        Verb::New { name, json } => new_note(name.as_deref(), json, ctx),
        Verb::Rename {
            note,
            new_name,
            json,
        } => rename_note(&note, &new_name, json, ctx),
        Verb::Trash { note, json } => trash_note(&note, json, ctx),
    }
}

// ---------------------------------------------------------------- the index

/// Why the note index could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IndexError {
    Absent(PathBuf),
    Older { db: i32, binary: i32 },
    Newer { db: i32, binary: i32 },
    Unreadable(String),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexError::Absent(path) => write!(
                f,
                "there is no note index at {}. Writ builds one the first time it runs.",
                path.display()
            ),
            IndexError::Older { db, binary } => write!(
                f,
                "the note index is at version {db} and this writ reads version {binary}. \
                 Writ brings it up to date the next time it runs."
            ),
            IndexError::Newer { db, binary } => write!(
                f,
                "the note index is at version {db}, past the version {binary} this writ reads. \
                 It was written by a newer Writ."
            ),
            IndexError::Unreadable(message) => {
                write!(f, "the note index could not be read: {message}")
            }
        }
    }
}

/// Opens the index read-only and checks it was written by this build's schema.
///
/// A version older than this binary's means the app has not migrated yet, and
/// the columns a verb reads may not be there. A newer one was written by a
/// build that knows more than this one. Both are refused rather than read
/// through the wrong column layout, and neither is repaired: migrating is the
/// app's, and only the app's.
fn open_index(db_path: &Path) -> Result<NotesIndexStore, IndexError> {
    if !db_path.is_file() {
        return Err(IndexError::Absent(db_path.to_path_buf()));
    }
    let store = NotesIndexStore::open_read_only(db_path)
        .map_err(|error| IndexError::Unreadable(error.to_string()))?;
    let db = store
        .schema_version()
        .map_err(|error| IndexError::Unreadable(error.to_string()))?;
    let binary = binary_schema_version();
    match db.cmp(&binary) {
        std::cmp::Ordering::Less => Err(IndexError::Older { db, binary }),
        std::cmp::Ordering::Greater => Err(IndexError::Newer { db, binary }),
        std::cmp::Ordering::Equal => Ok(store),
    }
}

// ------------------------------------------------------- naming a note

/// The file a `<note>` argument names, looked for on disk only.
///
/// In order: the argument as a path from the current directory, then a file of
/// that name in the notes folder, then that name with `.md` on it. A verb that
/// can also ask the index falls back to it when this finds nothing.
fn note_file(arg: &str, ctx: &Context) -> Option<PathBuf> {
    let given = Path::new(arg);
    let from_cwd = if given.is_absolute() {
        given.to_path_buf()
    } else {
        ctx.cwd.join(given)
    };
    [
        from_cwd,
        ctx.notes_dir.join(arg),
        ctx.notes_dir.join(format!("{arg}.{NOTE_EXTENSION}")),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// The index key of the note a read verb was asked about.
///
/// A path on disk keys directly. A name that is not a path is put to the index,
/// which answers with every note that name could mean: none is a failure, one
/// is the answer, and more than one is refused with the list, because picking
/// one is what quietly reads the wrong note.
fn note_key(arg: &str, store: &NotesIndexStore, ctx: &Context) -> Result<String, String> {
    if let Some(path) = note_file(arg, ctx) {
        return Ok(notes_index::index_key(&path));
    }
    let candidates = store
        .candidate_paths(arg)
        .map_err(|error| format!("the note index could not be read: {error}"))?;
    match candidates.len() {
        0 => Err(format!("no note called {arg}")),
        1 => Ok(candidates.into_iter().next().unwrap_or_default()),
        _ => Err(format!(
            "{arg} names more than one note:\n{}",
            candidates
                .iter()
                .map(|path| format!("  {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

// ----------------------------------------------------------- read verbs

/// A document a read verb produced, in both of its forms.
struct Document {
    human: String,
    json: serde_json::Value,
    /// Set when the answer is empty for want of a read rather than for want of
    /// facts, so an empty list is never taken at face value.
    caveat: Option<String>,
}

/// The shared shape of the four read verbs: open the index, name the note,
/// refuse a note it does not hold, then render.
fn read_verb(
    arg: &str,
    json: bool,
    ctx: &Context,
    build: fn(&NotesIndexStore, &str, IndexedBy) -> Result<Document, String>,
) -> Outcome {
    let store = match open_index(&ctx.db_path) {
        Ok(store) => store,
        Err(error) => return Outcome::failed(error.to_string()),
    };
    let key = match note_key(arg, &store, ctx) {
        Ok(key) => key,
        Err(message) => return Outcome::failed(message),
    };
    let indexed_by = match store.indexed_by(&key) {
        Ok(Some(indexed_by)) => indexed_by,
        Ok(None) => {
            return Outcome::failed(format!(
                "the note index does not hold {key}. It picks the note up on the next pass over \
                 the notes folder."
            ))
        }
        Err(error) => return Outcome::failed(format!("the note index could not be read: {error}")),
    };

    let document = match build(&store, &key, indexed_by) {
        Ok(document) => document,
        Err(message) => return Outcome::failed(message),
    };

    if json {
        let mut object = document.json;
        if let Some(map) = object.as_object_mut() {
            map.insert("note".into(), key.clone().into());
            map.insert("indexed_by".into(), indexed_by.as_str().into());
        }
        return Outcome::ok(format!("{object}\n"));
    }
    let outcome = Outcome::ok(document.human);
    match document.caveat {
        Some(caveat) => outcome.with_note(caveat),
        None => outcome,
    }
}

/// The one sentence a name-only row earns, said once per verb that an empty
/// answer would otherwise mislead.
fn name_only_caveat(indexed_by: IndexedBy) -> Option<String> {
    match indexed_by {
        IndexedBy::Name => {
            Some("this note has no data on this machine, so nothing was read out of it".to_string())
        }
        IndexedBy::Content => None,
    }
}

fn links_document(
    store: &NotesIndexStore,
    key: &str,
    indexed_by: IndexedBy,
) -> Result<Document, String> {
    let rows = store
        .links_from(key)
        .map_err(|error| format!("the note index could not be read: {error}"))?;

    let mut human = String::new();
    let mut json = Vec::new();
    for row in rows {
        let (status, path, candidates) = match row.to_path {
            Some(path) => ("resolved", Some(path), Vec::new()),
            None => match store.resolve_link(key, &row.to_target) {
                Ok(Resolution::Resolved(path)) => ("resolved", Some(path), Vec::new()),
                Ok(Resolution::Ambiguous(candidates)) => ("ambiguous", None, candidates),
                Ok(Resolution::Missing) => ("unresolved", None, Vec::new()),
                Err(error) => {
                    return Err(format!("the note index could not be read: {error}"));
                }
            },
        };
        let shown = match (&path, candidates.as_slice()) {
            (Some(path), _) => path.clone(),
            (None, []) => String::new(),
            (None, candidates) => candidates.join(", "),
        };
        human.push_str(&record([
            &row.line.to_string(),
            &row.col.to_string(),
            &row.kind,
            status,
            &row.to_target,
            &shown,
        ]));
        json.push(serde_json::json!({
            "target": row.to_target,
            "kind": row.kind,
            "line": row.line,
            "col": row.col,
            "status": status,
            "path": path,
            "candidates": candidates,
        }));
    }

    Ok(Document {
        human,
        json: serde_json::json!({ "links": json }),
        caveat: name_only_caveat(indexed_by),
    })
}

fn backlinks_document(
    store: &NotesIndexStore,
    key: &str,
    _indexed_by: IndexedBy,
) -> Result<Document, String> {
    let rows = store
        .backlinks(key)
        .map_err(|error| format!("the note index could not be read: {error}"))?;

    let mut human = String::new();
    let mut json = Vec::new();
    for row in rows {
        human.push_str(&record([
            &row.from_path,
            &row.from_name,
            &row.kind,
            &row.line.to_string(),
            &row.col.to_string(),
            row.certainty.as_str(),
            &row.to_target,
            row.alias.as_deref().unwrap_or(""),
            &row.context,
        ]));
        json.push(serde_json::json!({
            "from_path": row.from_path,
            "from_name": row.from_name,
            "kind": row.kind,
            "line": row.line,
            "col": row.col,
            "certainty": row.certainty.as_str(),
            "target": row.to_target,
            "alias": row.alias,
            "context": row.context,
        }));
    }

    // A note the index holds by name alone still has backlinks: they are
    // written in other notes. No caveat here.
    Ok(Document {
        human,
        json: serde_json::json!({ "backlinks": json }),
        caveat: None,
    })
}

fn properties_document(
    store: &NotesIndexStore,
    key: &str,
    indexed_by: IndexedBy,
) -> Result<Document, String> {
    let facts = store
        .facts(key)
        .map_err(|error| format!("the note index could not be read: {error}"))?;

    let mut human = String::new();
    let mut json = Vec::new();
    for (key, value) in facts.properties {
        human.push_str(&record([&key, &value]));
        json.push(serde_json::json!({
            "key": key,
            "value": serde_json::from_str::<serde_json::Value>(&value)
                .unwrap_or_else(|_| serde_json::Value::String(value.clone())),
        }));
    }

    Ok(Document {
        human,
        json: serde_json::json!({ "properties": json }),
        caveat: name_only_caveat(indexed_by),
    })
}

fn note_tags_document(
    store: &NotesIndexStore,
    key: &str,
    indexed_by: IndexedBy,
) -> Result<Document, String> {
    let facts = store
        .facts(key)
        .map_err(|error| format!("the note index could not be read: {error}"))?;

    let mut human = String::new();
    let mut json = Vec::new();
    for (tag, line) in facts.tags {
        human.push_str(&record([&tag, &line.to_string()]));
        json.push(serde_json::json!({ "tag": tag, "line": line }));
    }

    Ok(Document {
        human,
        json: serde_json::json!({ "tags": json }),
        caveat: name_only_caveat(indexed_by),
    })
}

/// `writ tags` with no note: every tag in the notes folder, with a note count.
fn folder_tags(json: bool, ctx: &Context) -> Outcome {
    let store = match open_index(&ctx.db_path) {
        Ok(store) => store,
        Err(error) => return Outcome::failed(error.to_string()),
    };
    let rows = match store.all_tags() {
        Ok(rows) => rows,
        Err(error) => return Outcome::failed(format!("the note index could not be read: {error}")),
    };

    if json {
        let tags: Vec<serde_json::Value> = rows
            .iter()
            .map(|(tag, notes)| serde_json::json!({ "tag": tag, "notes": notes }))
            .collect();
        let document = serde_json::json!({
            "note": serde_json::Value::Null,
            "notes_folder": ctx.notes_dir.to_string_lossy(),
            "tags": tags,
        });
        return Outcome::ok(format!("{document}\n"));
    }

    let mut human = String::new();
    for (tag, notes) in rows {
        human.push_str(&record([&tag, &notes.to_string()]));
    }
    Outcome::ok(human)
}

/// One tab-separated record, newline-terminated.
///
/// A tab or a line break inside a field would end the record early, so each is
/// replaced with a space. `--json` carries the field as it is.
fn record<const N: usize>(fields: [&str; N]) -> String {
    let mut line = fields
        .iter()
        .map(|field| {
            field
                .replace(['\t', '\n', '\r'], " ")
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\t");
    line.push('\n');
    line
}

// -------------------------------------------------------- writing verbs

/// The names `dir` already holds, lowercased the way the dedupe compares them.
fn taken_names(dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect()
}

/// Prints one path, as a bare line or as the documented JSON document.
fn path_outcome(json: bool, path: &Path, previous: Option<&Path>) -> Outcome {
    if json {
        let mut document = serde_json::json!({ "note": path.to_string_lossy() });
        if let Some(previous) = previous {
            if let Some(map) = document.as_object_mut() {
                map.insert("previous_path".into(), previous.to_string_lossy().into());
            }
        }
        return Outcome::ok(format!("{document}\n"));
    }
    Outcome::ok(format!("{}\n", path.display()))
}

/// Creates an empty note in the notes folder and prints its path.
///
/// The name goes through the same sanitiser and the same Finder-style dedupe
/// the app uses, so a note created here and one created in the window are named
/// by one rule. A name that survives sanitising to nothing is dated, which is
/// what an untitled note is called.
fn new_note(name: Option<&str>, json: bool, ctx: &Context) -> Outcome {
    if let Err(error) = std::fs::create_dir_all(&ctx.notes_dir) {
        return Outcome::failed(format!(
            "cannot create {}: {error}",
            ctx.notes_dir.display()
        ));
    }
    let stem = note_file_stem(name.unwrap_or(""), ctx.now);
    let file = dedupe_file_name(&stem, NOTE_EXTENSION, &taken_names(&ctx.notes_dir));
    let path = ctx.notes_dir.join(file);
    if let Err(error) = std::fs::write(&path, "") {
        return Outcome::failed(format!("cannot write {}: {error}", path.display()));
    }
    path_outcome(json, &path, None)
}

/// Renames a note in place through `note_ops::rename_note`.
///
/// No stamp is passed. The guard exists to keep the app from reading its own
/// write back as somebody else's; from another process there is nothing to
/// suppress, and the app's watcher *should* see this rename as the outside
/// change it is. No disk state is passed either: this process holds no record
/// of what the file last looked like, so there is nothing to compare against.
///
/// Links naming the note by its old name are not rewritten.
fn rename_note(arg: &str, new_name: &str, json: bool, ctx: &Context) -> Outcome {
    let Some(from) = note_file(arg, ctx) else {
        return Outcome::failed(format!("no note called {arg}"));
    };
    match note_ops::rename_note(&from, new_name, None, None) {
        Ok(to) => path_outcome(json, &to, Some(&from)),
        Err(error) => Outcome::failed(format!("cannot rename {}: {error}", from.display())),
    }
}

/// Moves a note to the operating system's trash, where it stays recoverable.
fn trash_note(arg: &str, json: bool, ctx: &Context) -> Outcome {
    let Some(path) = note_file(arg, ctx) else {
        return Outcome::failed(format!("no note called {arg}"));
    };
    match note_ops::trash_note(&path) {
        Ok(()) => path_outcome(json, &path, None),
        Err(error) => Outcome::failed(format!("cannot trash {}: {error}", path.display())),
    }
}

/// What a note is called, for a caller that has a path and wants the name a
/// link would use. Re-exported so the binary and the tests read one definition.
pub fn display_name(path: &Path) -> String {
    note_display_name(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_file_argument_is_not_a_verb() {
        assert!(parse(&os(&["note.md"])).is_none());
        assert!(parse(&os(&["./links"])).is_none());
        assert!(parse(&os(&[])).is_none());
    }

    #[test]
    fn every_listed_verb_parses() {
        for name in VERB_NAMES {
            assert!(
                parse(&os(&[name, "a", "b"])).is_some(),
                "{name} was not read as a verb"
            );
        }
    }

    #[test]
    fn a_read_verb_takes_a_note_and_the_json_flag() {
        assert_eq!(
            parse(&os(&["links", "Note", "--json"])).unwrap().unwrap(),
            Verb::Links {
                note: "Note".to_string(),
                json: true
            }
        );
        assert_eq!(
            parse(&os(&["links", "--json", "Note"])).unwrap().unwrap(),
            Verb::Links {
                note: "Note".to_string(),
                json: true
            }
        );
        assert_eq!(
            parse(&os(&["backlinks", "Note"])).unwrap().unwrap(),
            Verb::Backlinks {
                note: "Note".to_string(),
                json: false
            }
        );
    }

    #[test]
    fn tags_and_new_take_an_optional_argument() {
        assert_eq!(
            parse(&os(&["tags"])).unwrap().unwrap(),
            Verb::Tags {
                note: None,
                json: false
            }
        );
        assert_eq!(
            parse(&os(&["tags", "Note"])).unwrap().unwrap(),
            Verb::Tags {
                note: Some("Note".to_string()),
                json: false
            }
        );
        assert_eq!(
            parse(&os(&["new"])).unwrap().unwrap(),
            Verb::New {
                name: None,
                json: false
            }
        );
        assert_eq!(
            parse(&os(&["new", "Ideas", "--json"])).unwrap().unwrap(),
            Verb::New {
                name: Some("Ideas".to_string()),
                json: true
            }
        );
    }

    #[test]
    fn rename_takes_two_names() {
        assert_eq!(
            parse(&os(&["rename", "Old", "New"])).unwrap().unwrap(),
            Verb::Rename {
                note: "Old".to_string(),
                new_name: "New".to_string(),
                json: false
            }
        );
        assert_eq!(
            parse(&os(&["rename", "Old"])).unwrap().unwrap_err(),
            UsageError::MissingArgument {
                verb: "rename".to_string(),
                what: "a new name".to_string()
            }
        );
    }

    #[test]
    fn a_read_verb_with_no_note_is_a_usage_error() {
        assert_eq!(
            parse(&os(&["links"])).unwrap().unwrap_err(),
            UsageError::MissingArgument {
                verb: "links".to_string(),
                what: "a note".to_string()
            }
        );
    }

    #[test]
    fn extra_arguments_are_a_usage_error() {
        assert_eq!(
            parse(&os(&["links", "a", "b"])).unwrap().unwrap_err(),
            UsageError::TooManyArguments {
                verb: "links".to_string()
            }
        );
        assert_eq!(
            parse(&os(&["rename", "a", "b", "c"])).unwrap().unwrap_err(),
            UsageError::TooManyArguments {
                verb: "rename".to_string()
            }
        );
    }

    #[test]
    fn an_unknown_flag_is_a_usage_error() {
        assert_eq!(
            parse(&os(&["tags", "--yaml"])).unwrap().unwrap_err(),
            UsageError::UnknownFlag {
                verb: "tags".to_string(),
                flag: "--yaml".to_string()
            }
        );
    }

    #[test]
    fn a_record_keeps_its_fields_on_one_line() {
        assert_eq!(record(["a", "b", "c"]), "a\tb\tc\n");
        assert_eq!(record(["a\tb", "c\nd"]), "a b\tc d\n");
        assert_eq!(record([""; 2]), "\t\n");
    }

    #[test]
    fn the_usage_text_names_every_verb() {
        let usage = usage();
        for name in VERB_NAMES {
            assert!(usage.contains(*name), "usage does not mention {name}");
        }
    }

    #[test]
    fn the_usage_text_uses_plain_words() {
        let usage = usage().to_lowercase();
        for banned in ["vault", "buffer", "scratchpad", "second brain"] {
            assert!(!usage.contains(banned), "usage says {banned}");
        }
    }

    #[test]
    fn a_name_only_row_earns_a_caveat_and_a_read_row_does_not() {
        assert!(name_only_caveat(IndexedBy::Name).is_some());
        assert!(name_only_caveat(IndexedBy::Content).is_none());
    }

    #[test]
    fn every_index_failure_says_what_is_wrong_in_plain_words() {
        for error in [
            IndexError::Absent(PathBuf::from("/a/writ.db")),
            IndexError::Older { db: 41, binary: 42 },
            IndexError::Newer { db: 43, binary: 42 },
            IndexError::Unreadable("disk error".to_string()),
        ] {
            let message = error.to_string();
            assert!(!message.is_empty());
            assert!(
                !message.to_lowercase().contains("vault"),
                "{message} says vault"
            );
        }
    }

    #[test]
    fn a_note_is_named_by_the_core_rule() {
        assert_eq!(display_name(Path::new("/notes/Ideas.md")), "Ideas");
    }
}
