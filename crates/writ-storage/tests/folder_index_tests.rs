//! The folder index a connected program reads (ADR-052): every indexed file
//! with its title, tags, resolved links and length, read from the index and
//! written nowhere.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_core::config::FileExtension;
use writ_core::notes::host::{Capability, HostError, IndexEntry, NoteHost};
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::note_host::NoteHostImpl;
use writ_storage::notes_index;

struct Fixture {
    dir: TempDir,
    notes: PathBuf,
    db: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(notes.join("Projects")).expect("notes folder");
    std::fs::write(
        notes.join("Plan.md"),
        "---\ntitle: The plan\ntags: [work]\n---\n# Heading\nSee [[Launch]] and [[Launch]] and [[Nowhere]] #ship #ship\n",
    )
    .expect("note");
    std::fs::write(
        notes.join("Projects/Launch.md"),
        "Intro\n## Launch day\nBack to [[Plan]]\n",
    )
    .expect("note");
    std::fs::write(notes.join("log.txt"), "plain words\n").expect("note");
    let db = dir.path().join("writ.db");
    let conn = open_database(&db).expect("open the database");
    run_migrations(&conn).expect("migrations");
    notes_index::reconcile(&conn, &notes, &|| false, &|_| false).expect("walk the folder");
    Fixture { dir, notes, db }
}

fn host(fixture: &Fixture, capabilities: &[Capability]) -> NoteHostImpl<'static> {
    NoteHostImpl::open(
        &fixture.notes,
        Some(&fixture.db),
        capabilities.iter().copied().collect(),
        FileExtension::Md,
    )
    .expect("open the host")
}

fn entry<'a>(entries: &'a [IndexEntry], root: &Path, relative: &str) -> &'a IndexEntry {
    let key = notes_index::index_key(&root.join(relative));
    entries
        .iter()
        .find(|entry| entry.path == key)
        .unwrap_or_else(|| panic!("{relative} is not in {entries:?}"))
}

/// Every entry under `root` with its length and modification time.
fn listing(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for item in std::fs::read_dir(&folder).expect("read folder") {
            let path = item.expect("entry").path();
            let meta = std::fs::metadata(&path).expect("metadata");
            if meta.is_dir() {
                stack.push(path.clone());
            }
            entries.push((path, meta.len(), meta.modified().expect("mtime")));
        }
    }
    entries.sort();
    entries
}

#[test]
fn every_file_is_listed_with_its_title_tags_links_and_length() {
    let fixture = fixture();
    let host = host(&fixture, &[Capability::ReadIndex]);
    let root = host.notes_root().to_path_buf();
    let entries = host.folder_index().expect("the index");
    assert_eq!(entries.len(), 3, "{entries:?}");

    let plan = entry(&entries, &root, "Plan.md");
    assert_eq!(plan.title, "The plan", "the frontmatter title comes first");
    assert!(plan.tags.contains(&"ship".to_string()), "{:?}", plan.tags);
    assert_eq!(
        plan.tags.iter().filter(|tag| *tag == "ship").count(),
        1,
        "a tag is listed once"
    );
    assert_eq!(
        plan.links,
        vec![notes_index::index_key(&root.join("Projects/Launch.md"))],
        "each resolved link once, and none to a file that is not there"
    );
    assert_eq!(
        plan.bytes,
        std::fs::metadata(root.join("Plan.md")).expect("size").len()
    );

    let launch = entry(&entries, &root, "Projects/Launch.md");
    assert_eq!(launch.title, "Launch day", "the first heading stands in");

    let log = entry(&entries, &root, "log.txt");
    assert_eq!(
        log.title, "log.txt",
        "the name Writ shows stands in for a file with neither"
    );
}

#[test]
fn reading_the_index_leaves_the_folder_and_the_database_as_they_were() {
    let fixture = fixture();
    let database = |db: &Path| {
        let meta = std::fs::metadata(db).expect("writ.db");
        (meta.len(), meta.modified().expect("mtime"))
    };
    let notes_before = listing(&fixture.notes);
    let db_before = database(&fixture.db);
    host(&fixture, &[Capability::ReadIndex])
        .folder_index()
        .expect("the index");
    assert_eq!(listing(&fixture.notes), notes_before);
    assert_eq!(database(&fixture.db), db_before);
    assert!(fixture.dir.path().join("writ.db").is_file());
}

#[test]
fn the_index_needs_its_capability_and_an_index() {
    let fixture = fixture();
    assert!(matches!(
        host(&fixture, &[Capability::ListNotes]).folder_index(),
        Err(HostError::NotPermitted { .. })
    ));
    let without = NoteHostImpl::open(
        &fixture.notes,
        None,
        [Capability::ReadIndex].into_iter().collect(),
        FileExtension::Md,
    )
    .expect("open the host");
    assert_eq!(without.folder_index(), Err(HostError::IndexUnavailable));
}
