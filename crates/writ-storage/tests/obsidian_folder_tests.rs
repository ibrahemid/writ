//! A folder written in Obsidian, indexed once and read back whole.
//!
//! The rules each piece of this rests on are tested next door: links in
//! `writ_core::notes::links`, facts in `writ_core::notes::facts`, rows in
//! `notes_index_facts_tests`, the graph in `notes_index_graph_tests`. What is
//! tested here is the folder a person actually arrives with — nested folders,
//! a `.obsidian` settings tree, a `.trash`, attachments, aliases, heading
//! targets, frontmatter and inline tags, a name two folders answer to — going
//! through one reconcile with nothing lost and nothing guessed (ADR-034,
//! ADR-036).
//!
//! The fixture is copied to a temporary folder before the walk. The walk reads
//! git ignore files wherever it runs, so a checkout is not a place to index
//! from: a developer's global ignore rules would decide what the test sees.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_storage::notes_index::{self, GraphRows, NoteFactsRow, NotesIndexStore};

/// The folder as it was written in the other editor.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/obsidian-folder"
);

/// Copies `from` to `to`, dot-named entries included: the two folders this
/// test is about both start with a dot, and a copy that skipped them would
/// pass every assertion below for the wrong reason.
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
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open_database");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);

    let root = dir.path().join("notes");
    copy_tree(Path::new(FIXTURE), &root);
    assert!(
        root.join(".obsidian/app.json").is_file(),
        "the copy has to carry the settings folder for its absence to mean anything"
    );
    assert!(
        root.join(".obsidian/plugins/table-editor/main.js")
            .is_file(),
        "the copy has to carry the plugin tree"
    );
    assert!(
        root.join(".trash/Deleted Note.md").is_file(),
        "the copy has to carry the deleted note"
    );
    assert!(root.join("attachments/a.png").is_file(), "and the image");

    let index = NotesIndexStore::open(&db_path).expect("index");
    index
        .reconcile(&root, &|| false, &|_| false)
        .expect("reconcile");
    (dir, root, index)
}

fn key(root: &Path, relative: &str) -> String {
    notes_index::index_key(&root.join(relative))
}

fn indexed_paths(index: &NotesIndexStore) -> Vec<String> {
    let mut paths: Vec<String> = index
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|(path, ..)| path)
        .collect();
    paths.sort();
    paths
}

fn facts(index: &NotesIndexStore, root: &Path, relative: &str) -> NoteFactsRow {
    index.facts(&key(root, relative)).expect("facts")
}

fn edge_count(rows: &GraphRows, from: &str, to: &str) -> Option<usize> {
    rows.edges
        .iter()
        .find(|edge| edge.from_path == from && edge.to_path == to)
        .map(|edge| edge.count)
}

#[test]
fn the_settings_folder_and_the_trash_contribute_no_files() {
    let (_dir, root, index) = opened();

    assert_eq!(
        indexed_paths(&index),
        vec![
            key(&root, "Alpha.md"),
            key(&root, "Daily/2026-09-01.md"),
            key(&root, "Daily/Meeting.md"),
            key(&root, "Index.md"),
            key(&root, "Projects/Alpha/Roadmap.md"),
            key(&root, "Projects/Meeting.md"),
        ],
        "only the notes are indexed: not the settings tree, not the trash, not the image"
    );
    let backlinks = index.backlinks(&key(&root, "Alpha.md")).expect("backlinks");
    assert_eq!(
        backlinks.len(),
        6,
        "four links from the index note, one from the roadmap, one from the day, \
         and none from the deleted note"
    );
    assert!(
        backlinks
            .iter()
            .all(|row| !row.from_path.contains(".trash")),
        "a link written in a deleted note is not a backlink"
    );
    assert!(
        !index
            .all_tags()
            .expect("all_tags")
            .iter()
            .any(|(tag, _)| tag == "trashed"),
        "a tag written in a deleted note is not one of the folder's tags"
    );
}

#[test]
fn every_link_reaches_the_note_it_names() {
    let (_dir, root, index) = opened();

    let links = index.links_from(&key(&root, "Index.md")).expect("links");
    let written: Vec<(&str, Option<&str>)> = links
        .iter()
        .map(|link| (link.to_target.as_str(), link.to_path.as_deref()))
        .collect();
    let alpha = key(&root, "Alpha.md");
    let roadmap = key(&root, "Projects/Alpha/Roadmap.md");
    assert_eq!(
        written,
        vec![
            ("Alpha", Some(alpha.as_str())),
            ("Projects/Alpha/Roadmap", Some(roadmap.as_str())),
            ("Alpha", Some(alpha.as_str())),
            ("Alpha", Some(alpha.as_str())),
            ("Meeting", None),
            ("Nowhere", None),
            ("attachments/a.png", None),
            ("Alpha", Some(alpha.as_str())),
        ],
        "a link by name, one by path, one with a label, one to a heading, \
         one that two notes answer to, one that nothing answers to, an image \
         and a note embedded in the page"
    );
    assert_eq!(
        links[0].line, 13,
        "a link carries the line it is written on"
    );

    let by_path = index
        .links_from(&key(&root, "Projects/Alpha/Roadmap.md"))
        .expect("links");
    assert_eq!(
        by_path[1].to_path.as_deref(),
        Some(key(&root, "Daily/Meeting.md").as_str()),
        "a folder in front of the name picks one of the two notes that answer to it"
    );
}

#[test]
fn a_name_two_folders_answer_to_is_never_guessed() {
    let (_dir, root, index) = opened();

    let resolution = index
        .resolve_link(&key(&root, "Index.md"), "Meeting")
        .expect("resolve");
    assert_eq!(
        resolution,
        writ_core::notes::links::Resolution::Ambiguous(vec![
            key(&root, "Daily/Meeting.md"),
            key(&root, "Projects/Meeting.md"),
        ]),
        "both notes come back for the reader to pick between"
    );

    for relative in ["Daily/Meeting.md", "Projects/Meeting.md"] {
        let ambiguous = index
            .backlinks(&key(&root, relative))
            .expect("backlinks")
            .into_iter()
            .find(|row| row.to_target == "Meeting")
            .expect("the link is listed under both notes");
        assert_eq!(
            ambiguous.certainty,
            notes_index::BacklinkCertainty::Ambiguous
        );
        assert_eq!(ambiguous.candidates.len(), 1, "and names the other one");
    }
}

#[test]
fn a_label_and_a_heading_survive_the_walk() {
    let (_dir, root, index) = opened();

    let alpha = key(&root, "Alpha.md");
    let labelled = index
        .backlinks(&alpha)
        .expect("backlinks")
        .into_iter()
        .find(|row| row.alias.is_some())
        .expect("the labelled link");
    assert_eq!(labelled.alias.as_deref(), Some("the alpha plan"));
    assert_eq!(labelled.from_name, "Index");
    assert_eq!(labelled.line, 15);

    assert_eq!(
        index
            .resolve_link(&key(&root, "Index.md"), "Alpha#Scope")
            .expect("resolve"),
        writ_core::notes::links::Resolution::Resolved(alpha.clone()),
    );
    assert_eq!(
        index.heading_line(&alpha, "scope").expect("heading line"),
        Some(10),
        "the heading the link points at is the line the reader lands on"
    );
}

#[test]
fn tags_come_from_the_frontmatter_and_the_body_and_not_from_a_fence() {
    let (_dir, root, index) = opened();

    assert_eq!(
        facts(&index, &root, "Index.md").tags,
        vec![
            ("reading".to_string(), 3),
            ("project/alpha".to_string(), 3),
            ("daily".to_string(), 11),
            ("project/alpha".to_string(), 11),
        ],
        "the frontmatter list and the body tags, each with its line, and \
         `project/alpha` as one tag rather than two"
    );
    assert_eq!(
        facts(&index, &root, "Alpha.md").tags,
        vec![("project/alpha".to_string(), 4), ("reading".to_string(), 5),],
        "a frontmatter list written as items carries the line of each item"
    );
    assert!(
        !index
            .all_tags()
            .expect("all_tags")
            .iter()
            .any(|(tag, _)| tag == "not-a-tag"),
        "a tag written inside a fence is an example, not a tag"
    );
    assert_eq!(
        index.all_tags().expect("all_tags"),
        vec![
            ("daily".to_string(), 4),
            ("project/alpha".to_string(), 3),
            ("reading".to_string(), 2),
        ],
    );
    assert_eq!(
        index.paths_for_tag("project/alpha").expect("paths"),
        vec![
            key(&root, "Alpha.md"),
            key(&root, "Index.md"),
            key(&root, "Projects/Alpha/Roadmap.md"),
        ],
        "a nested tag is matched whole"
    );
}

#[test]
fn frontmatter_properties_keep_their_scalar_list_and_nested_map() {
    let (_dir, root, index) = opened();

    let properties = facts(&index, &root, "Index.md").properties;
    assert_eq!(
        properties,
        vec![
            ("title".to_string(), "\"Field notes\"".to_string()),
            (
                "tags".to_string(),
                "[\"reading\",\"project/alpha\"]".to_string()
            ),
            (
                "cover".to_string(),
                "\"  image: attachments/a.png\\n  fit: contain\"".to_string()
            ),
        ],
        "a map this parser does not model is kept as it was written rather than dropped"
    );
}

#[test]
fn headings_carry_their_level_text_line_and_slug() {
    let (_dir, root, index) = opened();

    let headings = facts(&index, &root, "Alpha.md").headings;
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[0].level, 1);
    assert_eq!(headings[0].text, "Alpha");
    assert_eq!(headings[0].line, 8);
    assert_eq!(headings[0].slug, "alpha");
    assert_eq!(headings[1].level, 2);
    assert_eq!(headings[1].text, "Scope");
    assert_eq!(headings[1].line, 10);
    assert_eq!(headings[1].slug, "scope");

    assert_eq!(
        facts(&index, &root, "Index.md").headings.len(),
        1,
        "the heading markers inside the fences are not headings"
    );
}

#[test]
fn the_graph_holds_every_note_and_every_resolved_link() {
    let (_dir, root, index) = opened();

    let rows = index.graph(&root).expect("graph");
    let nodes: Vec<(&str, &str)> = rows
        .nodes
        .iter()
        .map(|node| (node.name.as_str(), node.folder.as_str()))
        .collect();
    assert_eq!(
        nodes,
        vec![
            ("Alpha", ""),
            ("2026-09-01", "Daily"),
            ("Meeting", "Daily"),
            ("Index", ""),
            ("Roadmap", "Projects"),
            ("Meeting", "Projects"),
        ],
        "every note is a node, coloured by the folder it sits in"
    );

    let index_note = key(&root, "Index.md");
    let alpha = key(&root, "Alpha.md");
    let roadmap = key(&root, "Projects/Alpha/Roadmap.md");
    let meeting = key(&root, "Daily/Meeting.md");
    assert_eq!(
        edge_count(&rows, &index_note, &alpha),
        Some(4),
        "four links to one note are one edge with a weight"
    );
    assert_eq!(edge_count(&rows, &index_note, &roadmap), Some(1));
    assert_eq!(edge_count(&rows, &alpha, &index_note), Some(1));
    assert_eq!(edge_count(&rows, &roadmap, &alpha), Some(1));
    assert_eq!(edge_count(&rows, &roadmap, &meeting), Some(1));
    assert_eq!(
        edge_count(&rows, &index_note, &key(&root, "Projects/Meeting.md")),
        None,
        "the name two notes answer to is drawn to neither"
    );
    assert_eq!(rows.edges.len(), 6);
}

#[test]
fn walking_the_folder_a_second_time_changes_nothing() {
    let (_dir, root, index) = opened();

    let first: Vec<NoteFactsRow> = index
        .note_paths()
        .expect("note paths")
        .iter()
        .map(|path| index.facts(path).expect("facts"))
        .collect();
    let tags = index.all_tags().expect("all_tags");
    let graph = index.graph(&root).expect("graph");
    let paths = indexed_paths(&index);

    let outcome = index
        .reconcile(&root, &|| false, &|_| false)
        .expect("second reconcile");
    assert!(!outcome.cancelled);

    let second: Vec<NoteFactsRow> = index
        .note_paths()
        .expect("note paths")
        .iter()
        .map(|path| index.facts(path).expect("facts"))
        .collect();
    assert_eq!(second, first, "no row is written twice and none is lost");
    assert_eq!(index.all_tags().expect("all_tags"), tags);
    assert_eq!(index.graph(&root).expect("graph"), graph);
    assert_eq!(indexed_paths(&index), paths);
}
