//! The rename and the editor answer the same question the same way (ADR-034).
//!
//! A rename repoints the links that reach the renamed note, and the editor
//! opens the note a link reaches. Both answers come from `links::resolve`, and
//! both are only worth anything if they are the same answer: a rename that
//! repoints a link the editor would have opened elsewhere silently edits a
//! note nobody was pointing at.
//!
//! The two surfaces are reached the way the app reaches them —
//! `writ_core::notes::rename::rewrite_links` over the candidate list the
//! rename command builds, and `resolve_note_link_inner` against a real index —
//! so this is a seam test rather than one function compared with itself.
//!
//! The folder is the one that separates the two extension rules:
//! `one/Note.md` and `two/Note.md` answer to the same name, and `Note.md.md`
//! answers to a target that a second extension strip would read as `Note`.

use std::path::Path;

use tempfile::TempDir;
use writ_core::notes::links;
use writ_core::notes::rename::{rewrite_links, Rewrite};
use writ_storage::note_ops;
use writ_storage::notes_index::{self, NotesIndexStore};
use writ_tauri_lib::commands::note_index::resolve_note_link_inner;

/// The links every case runs, as they are written inside `[[…]]`.
/// The notes on disk, in the order a rename is tried over them.
const NOTES: [&str; 3] = ["one/Note.md", "two/Note.md", "Note.md.md"];

const TARGETS: [&str; 4] = ["Note", "Note.md", "Note.md.md", "one/Note"];

/// The note holding the one link written as `target`. One note per link, so a
/// rewrite names which link the rename repointed.
fn from_name(target: &str) -> String {
    format!("From {}.md", target.replace('/', "-"))
}

/// `root` plus a relative path, joined a component at a time so the spelling is
/// the platform's own.
fn at(root: &Path, relative: &str) -> std::path::PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn write_note(notes: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = at(notes, name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, body).expect("write note");
    path
}

/// The folder both surfaces are asked about, walked once.
fn folder() -> (TempDir, std::path::PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);

    let root = dir.path().join("notes");
    std::fs::create_dir_all(&root).expect("create notes dir");
    write_note(&root, "one/Note.md", "# One\n");
    write_note(&root, "two/Note.md", "# Two\n");
    write_note(&root, "Note.md.md", "# Long\n");
    for target in TARGETS {
        write_note(&root, &from_name(target), &format!("see [[{target}]]\n"));
    }

    let index = NotesIndexStore::open(&db_path).expect("index");
    index
        .reconcile(&root, &|| false, &|_| false)
        .expect("reconcile");
    (dir, root, index)
}

/// The notes the rename surface repoints this link to: every note whose rename
/// rewrites it. A link the rename leaves alone is one it would not repoint at
/// all, which is the answer an ambiguous link gets.
fn rename_answer(root: &Path, index: &NotesIndexStore, target: &str) -> Vec<String> {
    let from = notes_index::index_key(&at(root, &from_name(target)));
    let text = format!("see [[{target}]]\n");
    let mut candidates = index.note_paths().expect("note paths");
    candidates.sort_unstable();

    let mut reached = Vec::new();
    for note in NOTES {
        let key = notes_index::index_key(&at(root, note));
        // What the rename command hands `rewrite_links` beside the index's own
        // list: the files a link could reach that the index does not hold.
        let unindexed: Vec<String> = note_ops::files_named(root, &links::candidate_name_keys(&key))
            .into_iter()
            .map(|file| notes_index::index_key(&file))
            .filter(|other| *other != key && !candidates.contains(other))
            .collect();
        assert!(
            unindexed.is_empty(),
            "every note here is indexed, so the walk leaves nothing over: {unindexed:?}"
        );
        if let Rewrite::Rewritten(_) =
            rewrite_links(&text, &from, &key, "Renamed", &candidates, &unindexed)
        {
            reached.push(key);
        }
    }
    reached.sort_unstable();
    reached
}

/// The note the editor opens for the same link, or the notes it offers when it
/// will not pick one.
fn editor_answer(root: &Path, index: &NotesIndexStore, target: &str) -> (String, Vec<String>) {
    let from = at(root, &from_name(target));
    let dto = resolve_note_link_inner(index, from.to_str().expect("utf-8 path"), target)
        .expect("resolve");
    let mut named: Vec<String> = dto.path.into_iter().chain(dto.candidates).collect();
    named.sort_unstable();
    (dto.status, named)
}

#[test]
fn the_rename_and_the_editor_name_the_same_note_for_the_same_link() {
    let (_dir, root, index) = folder();

    // `[[Note]]` and `[[Note.md]]` are the same name written two ways, and two
    // notes answer to it. `[[Note.md.md]]` is the shape a second extension
    // strip reads as `Note`, which would put it in with those two instead of on
    // the note carrying both extensions. A folder settles the first case.
    for (target, status, named) in [
        ("Note", "ambiguous", &["one/Note.md", "two/Note.md"][..]),
        ("Note.md", "ambiguous", &["one/Note.md", "two/Note.md"][..]),
        ("Note.md.md", "resolved", &["Note.md.md"][..]),
        ("one/Note", "resolved", &["one/Note.md"][..]),
    ] {
        let mut expected: Vec<String> = named
            .iter()
            .map(|note| notes_index::index_key(&at(&root, note)))
            .collect();
        expected.sort_unstable();

        let (editor_status, editor_named) = editor_answer(&root, &index, target);
        assert_eq!(
            editor_status, status,
            "the editor's answer for [[{target}]]"
        );
        assert_eq!(editor_named, expected, "the notes [[{target}]] names");

        // The rename repoints a link to the note the editor opens, and leaves
        // an ambiguous one for the user rather than guessing between the notes
        // the editor would have offered.
        let repointed = match status {
            "resolved" => expected,
            _ => Vec::new(),
        };
        assert_eq!(
            rename_answer(&root, &index, target),
            repointed,
            "the rename and the editor disagree about [[{target}]]"
        );
    }
}
