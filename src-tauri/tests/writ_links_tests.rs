//! What the app does with a `writ://` link (ADR-052): an `open` reaches only a
//! file the notes folder holds, a `new` asks for a tab, and neither writes.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tempfile::TempDir;
use writ_core::writ_link::LinkRefusal;
use writ_tauri_lib::links::{
    park_or_pass, parked, read_links, route_link, LinkDrafts, LinkError, LinkRoute,
};

struct Folder {
    dir: TempDir,
    root: PathBuf,
}

fn folder() -> Folder {
    let dir = TempDir::new().expect("temp dir");
    let root = writ_tauri_lib::security::canonicalize_root(dir.path())
        .expect("canonical temp dir")
        .join("Notes");
    std::fs::create_dir_all(root.join("Projects")).expect("folders");
    std::fs::write(root.join("Plan.md"), "one\ntwo\n").expect("note");
    std::fs::write(root.join("Projects").join("Launch Plan.md"), "launch").expect("note");
    std::fs::write(dir.path().join("secret.md"), "outside").expect("outside");
    Folder { dir, root }
}

/// Every entry under `root`, with its length, so a test can say nothing moved.
fn listing(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).expect("read folder") {
            let path = entry.expect("entry").path();
            let meta = std::fs::symlink_metadata(&path).expect("metadata");
            if meta.is_dir() {
                stack.push(path.clone());
            }
            entries.push((path, meta.len()));
        }
    }
    entries.sort();
    entries
}

fn spelled(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn an_open_link_reaches_the_file_it_names_at_its_line() {
    let folder = folder();
    assert_eq!(
        route_link(&folder.root, "writ://open?path=Plan.md&line=12"),
        Ok(LinkRoute::Open {
            path: spelled(folder.root.join("Plan.md")),
            line: Some(12),
        })
    );
    assert_eq!(
        route_link(&folder.root, "writ://open?path=Projects%2FLaunch%20Plan.md"),
        Ok(LinkRoute::Open {
            path: spelled(folder.root.join("Projects").join("Launch Plan.md")),
            line: None,
        })
    );
}

#[test]
fn a_path_out_of_the_folder_is_refused() {
    let folder = folder();
    let refusals = [
        (
            "writ://open?path=../secret.md",
            LinkError::Grammar(LinkRefusal::BadSegment),
        ),
        (
            "writ://open?path=%2E%2E%2Fsecret.md",
            LinkError::Grammar(LinkRefusal::BadSegment),
        ),
        (
            "writ://open?path=%2Fetc%2Fpasswd",
            LinkError::Grammar(LinkRefusal::NotRelative),
        ),
        ("writ://open?path=Missing.md", LinkError::NotInFolder),
        ("writ://open?path=Projects", LinkError::NotInFolder),
    ];
    for (link, refusal) in refusals {
        assert_eq!(route_link(&folder.root, link), Err(refusal), "{link}");
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_folder_is_refused() {
    let folder = folder();
    std::os::unix::fs::symlink(
        folder.dir.path().join("secret.md"),
        folder.root.join("Out.md"),
    )
    .expect("symlink");
    assert_eq!(
        route_link(&folder.root, "writ://open?path=Out.md"),
        Err(LinkError::NotInFolder)
    );
}

#[test]
fn an_unknown_verb_is_refused() {
    let folder = folder();
    for link in [
        "writ://save?path=Plan.md",
        "writ://write?path=Plan.md&text=x",
        "writ://delete?path=Plan.md",
    ] {
        assert_eq!(
            route_link(&folder.root, link),
            Err(LinkError::Grammar(LinkRefusal::UnknownVerb)),
            "{link}"
        );
    }
}

#[test]
fn a_new_link_asks_for_a_tab_and_names_no_file() {
    let folder = folder();
    assert_eq!(
        route_link(&folder.root, "writ://new?name=Launch%20notes"),
        Ok(LinkRoute::Draft(Some("Launch notes".to_string())))
    );
    assert_eq!(
        route_link(&folder.root, "writ://new"),
        Ok(LinkRoute::Draft(None))
    );
}

#[test]
fn following_links_leaves_the_folder_as_it_was() {
    let folder = folder();
    let before = listing(folder.dir.path());
    let followed = read_links(
        &folder.root,
        &[
            "writ://open?path=Plan.md&line=2".to_string(),
            "writ://new?name=Plan.md".to_string(),
            "writ://new?name=Brand%20new".to_string(),
            "writ://open?path=../secret.md".to_string(),
            "writ://save?path=Plan.md".to_string(),
        ],
    );
    assert_eq!(listing(folder.dir.path()), before);

    let plan = spelled(folder.root.join("Plan.md"));
    assert_eq!(followed.opens.paths, vec![plan.clone()]);
    assert_eq!(followed.opens.lines.get(&plan), Some(&2));
    assert_eq!(
        followed.drafts,
        vec![Some("Plan.md".to_string()), Some("Brand new".to_string())]
    );
}

#[test]
fn a_new_tab_asked_for_before_the_frontend_is_ready_waits_for_it() {
    let drafts = LinkDrafts::default();
    let ready = AtomicBool::new(false);
    assert_eq!(
        park_or_pass(&drafts, &ready, vec![Some("Plan".to_string())]),
        None
    );
    assert_eq!(parked(&drafts), vec![Some("Plan".to_string())]);

    ready.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        park_or_pass(&drafts, &ready, vec![None]),
        Some(vec![None]),
        "once the frontend is ready a new tab goes straight to it"
    );
}
