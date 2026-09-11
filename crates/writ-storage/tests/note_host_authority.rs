//! One host, and a test that keeps the two consumers behind it.
//!
//! Both consumers reached the index and the write facade directly before the
//! host existed, and the bodies had already begun to diverge: one read a note's
//! size with a check the other did not. What stops the third copy is not the
//! module boundary, since both crates still depend on `writ-storage` for
//! everything else they do, but this test, which reads their sources and says
//! where each name is allowed to appear.
//!
//! Doc comments count. Prose naming the store is prose a reader will follow
//! back to it, so the comments say "the note host" instead.

use std::path::{Path, PathBuf};

/// The names a consumer of the surface does not reach for.
///
/// The index read model and the write facade are both the host's, and a
/// consumer naming either is a consumer doing the host's half itself.
const WITHHELD: &[&str] = &["NotesIndexStore", "guarded::"];

/// The workspace root, from this crate's manifest folder.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root is two folders up from this crate")
        .to_path_buf()
}

/// The file with its inline test module cut off.
///
/// A test may name whatever it is testing. The convention the workspace follows
/// is one `#[cfg(test)]` module at the end of the file, so everything from the
/// first one on is test code.
fn outside_tests(text: &str) -> &str {
    match text.find("#[cfg(test)]") {
        Some(at) => &text[..at],
        None => text,
    }
}

/// Every `.rs` file under `dir`, at any depth.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read a source folder") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            found.extend(sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    found
}

/// Each file under `root` that names one of [`WITHHELD`] outside its tests,
/// with the name it used, sorted.
fn reaching_past_the_host(root: &Path) -> Vec<String> {
    let mut named: Vec<String> = sources(root)
        .into_iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(&path).expect("read a source file");
            let shipped = outside_tests(&text).to_string();
            let name = path
                .file_name()
                .expect("a source file has a name")
                .to_string_lossy()
                .into_owned();
            WITHHELD
                .iter()
                .filter(move |withheld| shipped.contains(**withheld))
                .map(move |withheld| format!("{name}: {withheld}"))
        })
        .collect();
    named.sort();
    named
}

#[test]
fn the_tool_surface_reaches_notes_only_through_the_host() {
    let root = workspace_root().join("crates/writ-mcp/src");
    assert!(root.is_dir(), "{}", root.display());

    assert_eq!(
        reaching_past_the_host(&root),
        Vec::<String>::new(),
        "a tool that opens the index or the write facade itself is answering \
         \"may this client do that\" a second time; go through `NoteHostImpl`"
    );
}

#[test]
fn the_chat_pane_reaches_notes_only_through_the_host() {
    let chat = workspace_root().join("src-tauri/src/commands/chat.rs");
    let text = std::fs::read_to_string(&chat).expect("read the chat pane's commands");
    let shipped = outside_tests(&text);

    let named: Vec<&str> = WITHHELD
        .iter()
        .copied()
        .filter(|withheld| shipped.contains(withheld))
        .collect();
    assert!(
        named.is_empty(),
        "the pane reads and writes notes through the host it holds a permission set for; \
         found {named:?}"
    );
}

#[test]
fn the_check_reads_the_files_it_claims_to() {
    // A path that had gone stale would pass both assertions above by finding
    // nothing to read.
    let root = workspace_root().join("crates/writ-mcp/src");
    assert!(
        sources(&root).len() > 1,
        "the walk reached {} files",
        sources(&root).len()
    );
    let chat = workspace_root().join("src-tauri/src/commands/chat.rs");
    assert!(chat.is_file(), "{}", chat.display());
}
