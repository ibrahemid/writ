//! The Obsidian fixture folder read through the note commands.
//!
//! `obsidian_folder_tests` in `writ-storage` asserts the rows. This asserts
//! that what the editor is handed carries them: the same folder, the same one
//! walk, read back through `note_facts`, `note_backlinks`, `note_all_tags` and
//! `note_graph` and their DTOs (ADR-034, ADR-036).

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_storage::notes_index::{self, NotesIndexStore};
use writ_tauri_lib::commands::note_index::{
    note_all_tags_inner, note_backlinks_inner, note_facts_inner, note_graph_inner,
};

/// The folder as it was written in the other editor, shared with the storage
/// test so both layers answer for one corpus.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../crates/writ-storage/tests/fixtures/obsidian-folder"
);

/// Copies `from` to `to`, dot-named entries included: the settings folder and
/// the trash are what this is about.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create folder");
    for entry in std::fs::read_dir(from).expect("read folder") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

/// The fixture folder, copied out and walked once.
fn opened() -> (TempDir, PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);

    let root = dir.path().join("notes");
    copy_tree(Path::new(FIXTURE), &root);
    assert!(root.join(".obsidian/app.json").is_file());
    assert!(root.join(".trash/Deleted Note.md").is_file());

    let index = NotesIndexStore::open(&db_path).expect("index");
    index
        .reconcile(&root, &|| false, &|_| false)
        .expect("reconcile");
    (dir, root, index)
}

fn path_of(root: &Path, relative: &str) -> String {
    root.join(relative)
        .to_str()
        .expect("utf-8 path")
        .to_string()
}

fn key(root: &Path, relative: &str) -> String {
    notes_index::index_key(&root.join(relative))
}

#[test]
fn note_facts_hands_the_editor_the_links_tags_properties_and_headings() {
    let (_dir, root, index) = opened();

    let facts = note_facts_inner(&index, &path_of(&root, "Index.md")).expect("facts");

    let alpha = key(&root, "Alpha.md");
    assert_eq!(
        facts
            .links
            .iter()
            .filter(|link| link.to_path.as_deref() == Some(alpha.as_str()))
            .count(),
        4,
        "the link by name, the labelled one, the one to a heading and the embedded note"
    );
    assert!(
        facts
            .links
            .iter()
            .any(|link| link.to_target == "Meeting" && link.to_path.is_none()),
        "a name two notes answer to reaches the editor unresolved"
    );
    assert!(
        facts
            .links
            .iter()
            .any(|link| link.to_target == "Nowhere" && link.to_path.is_none()),
        "so does a name no note answers to"
    );
    assert_eq!(
        facts
            .tags
            .iter()
            .map(|tag| (tag.tag.as_str(), tag.line))
            .collect::<Vec<_>>(),
        vec![
            ("reading", 3),
            ("project/alpha", 3),
            ("daily", 11),
            ("project/alpha", 11),
        ],
    );
    assert_eq!(
        facts
            .properties
            .iter()
            .map(|property| property.key.as_str())
            .collect::<Vec<_>>(),
        vec!["title", "tags", "cover"],
    );
    assert_eq!(facts.headings.len(), 1);
    assert_eq!(facts.headings[0].slug, "index");

    let scope = note_facts_inner(&index, &path_of(&root, "Alpha.md")).expect("facts");
    assert_eq!(scope.headings[1].text, "Scope");
    assert_eq!(scope.headings[1].line, 10);
}

#[test]
fn note_backlinks_carries_the_label_and_the_notes_an_ambiguous_link_could_mean() {
    let (_dir, root, index) = opened();

    let backlinks = note_backlinks_inner(&index, &path_of(&root, "Alpha.md")).expect("backlinks");
    assert_eq!(backlinks.len(), 6);
    let labelled = backlinks
        .iter()
        .find(|row| row.alias.is_some())
        .expect("the labelled link");
    assert_eq!(labelled.alias.as_deref(), Some("the alpha plan"));
    assert_eq!(labelled.from_name, "Index");
    assert_eq!(labelled.certainty, "resolved");
    assert!(
        labelled.context.contains("the alpha plan"),
        "the sentence the link sits in comes with it"
    );
    assert!(
        backlinks
            .iter()
            .all(|row| !row.from_path.contains(".trash")),
        "a link written in a deleted note is not a backlink"
    );

    let ambiguous = note_backlinks_inner(&index, &path_of(&root, "Daily/Meeting.md"))
        .expect("backlinks")
        .into_iter()
        .find(|row| row.to_target == "Meeting")
        .expect("the ambiguous link is listed here too");
    assert_eq!(ambiguous.certainty, "ambiguous");
    assert_eq!(
        ambiguous.candidates,
        vec![key(&root, "Projects/Meeting.md")],
        "the other note it could mean is named rather than picked"
    );
}

#[test]
fn note_all_tags_counts_the_notes_carrying_each_tag() {
    let (_dir, _root, index) = opened();

    let tags = note_all_tags_inner(&index).expect("all tags");
    assert_eq!(
        tags.iter()
            .map(|tag| (tag.tag.as_str(), tag.count))
            .collect::<Vec<_>>(),
        vec![
            ("daily", 4),
            ("project/alpha", 4),
            ("reading", 2),
            ("work", 1),
        ],
        "a tag written in the frontmatter counts beside the ones in the body, \
         a comma-separated value holds both tags it names, and a nested tag is \
         one tag"
    );
}

#[test]
fn note_graph_draws_every_note_and_the_links_that_reached_one() {
    let (_dir, root, index) = opened();

    let graph = note_graph_inner(&index, &root).expect("graph");
    assert_eq!(graph.nodes.len(), 6);
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.folder == "Daily")
            .count(),
        2,
        "a node carries the folder it sits in"
    );

    let index_note = key(&root, "Index.md");
    let alpha = key(&root, "Alpha.md");
    let weight = graph
        .edges
        .iter()
        .find(|edge| edge.from_path == index_note && edge.to_path == alpha)
        .map(|edge| edge.count);
    assert_eq!(weight, Some(4));
    assert_eq!(graph.edges.len(), 6);
    assert!(
        graph
            .edges
            .iter()
            .all(|edge| edge.to_path != key(&root, "Projects/Meeting.md")),
        "a link two notes answer to is drawn to neither"
    );
}
