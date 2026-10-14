//! Gathering a context pack: the files a source names, read with read access
//! alone, with an open tab's text in place of its file's.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_core::prompt::pack::{SkipReason, MAX_PACK_FILES};
use writ_storage::context_pack::{
    build_context_pack, pack_paths, ContextPackError, LiveText, PackSource,
};
use writ_storage::notes_index::{self, NotesIndexStore};

fn write_file(root: &Path, name: &str, body: &[u8]) {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, body).expect("write file");
}

/// An index over a folder of files, walked once.
fn indexed(files: &[(&str, &str)]) -> (TempDir, PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open_database");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);

    let root = dir.path().join("Writ");
    std::fs::create_dir_all(&root).expect("create folder");
    for (name, body) in files {
        write_file(&root, name, body.as_bytes());
    }
    let root = std::fs::canonicalize(&root).expect("canonical");
    let index = NotesIndexStore::open(&db_path).expect("index");
    index
        .reconcile(&root, &|| false, &|_| false)
        .expect("reconcile");
    (dir, root, index)
}

fn path_of(root: &Path, name: &str) -> String {
    root.join(name).to_string_lossy().into_owned()
}

fn linked(root: &Path, name: &str) -> PackSource {
    PackSource::Linked {
        path: path_of(root, name),
    }
}

fn names(pack: &writ_core::prompt::pack::ContextPack) -> Vec<&str> {
    pack.files.iter().map(|file| file.path.as_str()).collect()
}

#[test]
fn a_file_brings_the_files_its_links_resolve_to_in_link_order() {
    let (_dir, root, index) = indexed(&[
        (
            "Plan.md",
            "See [[Budget]], then [[Team]] and [[Budget]] again, and [[Nowhere]].\n",
        ),
        ("Budget.md", "Ten thousand.\n"),
        ("Team.md", "Ada, Grace.\n"),
        ("Other.md", "Not linked.\n"),
    ]);

    let pack = build_context_pack(&index, &root, &linked(&root, "Plan.md"), &[]).expect("pack");

    assert_eq!(names(&pack), ["Plan.md", "Budget.md", "Team.md"]);
    assert!(pack
        .text
        .starts_with("<file path=\"Plan.md\">\nSee [[Budget]]"));
    assert!(pack
        .text
        .contains("<file path=\"Budget.md\">\nTen thousand.\n</file>"));
    assert!(pack.skipped.is_empty());
    assert_eq!(pack.omitted, 0);
    assert!(pack.tokens > 0);
}

#[test]
fn an_open_tab_gives_its_text_in_place_of_the_file() {
    let (_dir, root, index) = indexed(&[
        ("Plan.md", "See [[Budget]].\n"),
        ("Budget.md", "Ten thousand.\n"),
    ]);
    let live = [LiveText {
        path: path_of(&root, "Budget.md"),
        text: "Twelve thousand, typed a moment ago.\n".into(),
    }];

    let pack = build_context_pack(&index, &root, &linked(&root, "Plan.md"), &live).expect("pack");

    assert!(pack.text.contains("Twelve thousand, typed a moment ago."));
    assert!(!pack.text.contains("Ten thousand."));
}

#[test]
fn a_tag_brings_the_files_carrying_it() {
    let (_dir, root, index) = indexed(&[
        ("One.md", "#launch notes\n"),
        ("Two.md", "#launch and #draft\n"),
        ("Three.md", "#draft\n"),
    ]);

    let source = PackSource::Tag {
        tag: "#launch".into(),
    };
    let pack = build_context_pack(&index, &root, &source, &[]).expect("pack");

    assert_eq!(names(&pack), ["One.md", "Two.md"]);
}

#[test]
fn a_folder_brings_its_files_and_the_folders_under_it_and_no_sibling() {
    let (_dir, root, index) = indexed(&[
        ("work/a.md", "a\n"),
        ("work/deep/b.md", "b\n"),
        ("workshop/c.md", "c\n"),
        ("top.md", "top\n"),
    ]);

    let source = PackSource::Folder {
        path: path_of(&root, "work"),
    };
    let pack = build_context_pack(&index, &root, &source, &[]).expect("pack");

    assert_eq!(names(&pack), ["work/a.md", "work/deep/b.md"]);
}

#[test]
fn the_folder_itself_is_a_folder_source() {
    let (_dir, root, index) = indexed(&[("a.md", "a\n"), ("sub/b.md", "b\n")]);

    let source = PackSource::Folder {
        path: root.to_string_lossy().into_owned(),
    };
    let pack = build_context_pack(&index, &root, &source, &[]).expect("pack");

    assert_eq!(names(&pack), ["a.md", "sub/b.md"]);
}

#[test]
fn a_source_outside_the_folder_is_refused() {
    let (dir, root, index) = indexed(&[("a.md", "a\n")]);
    let outside = dir.path().join("elsewhere.md");
    std::fs::write(&outside, "secret\n").expect("write");

    let file = PackSource::Linked {
        path: outside.to_string_lossy().into_owned(),
    };
    assert!(matches!(
        build_context_pack(&index, &root, &file, &[]),
        Err(ContextPackError::OutsideFolder { .. })
    ));
    let folder = PackSource::Folder {
        path: dir.path().to_string_lossy().into_owned(),
    };
    assert!(matches!(
        pack_paths(&index, &root, &folder),
        Err(ContextPackError::OutsideFolder { .. })
    ));
}

#[test]
fn a_file_it_cannot_read_is_listed_with_the_reason() {
    let (_dir, root, index) = indexed(&[
        ("Plan.md", "See [[Gone]] and [[Big]].\n"),
        ("Gone.md", "here at the walk\n"),
        ("Big.md", "small at the walk\n"),
    ]);
    std::fs::remove_file(root.join("Gone.md")).expect("remove");
    let big = vec![b'a'; (writ_core::notes::host::MAX_NOTE_BYTES + 1) as usize];
    write_file(&root, "Big.md", &big);

    let pack = build_context_pack(&index, &root, &linked(&root, "Plan.md"), &[]).expect("pack");

    assert_eq!(names(&pack), ["Plan.md"]);
    let skipped: Vec<(&str, SkipReason)> = pack
        .skipped
        .iter()
        .map(|file| (file.path.as_str(), file.reason))
        .collect();
    assert_eq!(
        skipped,
        [
            ("Gone.md", SkipReason::Unreadable),
            ("Big.md", SkipReason::TooLarge)
        ]
    );
}

#[test]
fn a_folder_past_the_file_limit_is_packed_to_it_and_the_rest_counted() {
    let files: Vec<(String, String)> = (0..MAX_PACK_FILES + 5)
        .map(|n| (format!("many/{n:04}.md"), format!("file {n}\n")))
        .collect();
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect();
    let (_dir, root, index) = indexed(&borrowed);

    let source = PackSource::Folder {
        path: path_of(&root, "many"),
    };
    let pack = build_context_pack(&index, &root, &source, &[]).expect("pack");

    assert_eq!(pack.files.len(), MAX_PACK_FILES);
    assert_eq!(pack.omitted, 5);
}

#[test]
fn the_source_file_keys_match_the_index() {
    let (_dir, root, index) = indexed(&[("Plan.md", "x\n")]);
    let paths = pack_paths(&index, &root, &linked(&root, "Plan.md")).expect("paths");
    assert_eq!(paths, [notes_index::index_key(&root.join("Plan.md"))]);
}
