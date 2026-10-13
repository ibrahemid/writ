//! Which files the index holds, and the walk over the notes folder that finds
//! them.

use std::path::Path;

use ignore::WalkBuilder;
use writ_core::file_ops::{classify_path, FileOpenMode};
use writ_core::workspace::{is_notes_ignored_name, path_has_notes_ignored_name};

/// macOS `SF_DATALESS`: the file has no local data behind it.
const SF_DATALESS: u32 = 0x4000_0000;

/// The walk over the notes folder: every file, hidden ones included, symlinks
/// never followed, and nothing pruned but the names
/// [`writ_core::workspace::is_notes_ignored_name`] answers for.
///
/// It is not the workspace search walk
/// ([`crate::workspace_search::build_walk`]). That one prunes build folders
/// and honours every `.gitignore`, the person's global one included, because
/// what it reads is a code checkout. A notes folder is writing: `vendor/` and
/// `build/` in it hold notes, and a global gitignore written for code
/// (`*.log`, `scratch/`) says nothing about which notes exist (ADR-028
/// section 7). The index walk, the listing a connected program reads and the
/// rename's search for same-named notes all walk with this, so they agree on
/// which notes there are.
pub(crate) fn build_notes_walk(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
        .parents(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false);
    builder.filter_entry(|entry| !is_notes_ignored_name(&entry.file_name().to_string_lossy()));
    builder
}

/// `true` when the filesystem reports the file has no local data: an iCloud (or
/// other provider) placeholder that reading would materialise.
///
/// macOS reads `SF_DATALESS` out of the stat flags, which is metadata only and
/// never opens the file. Every other platform answers `false`: no other
/// filesystem Writ supports reports the state, and guessing would skip real
/// files.
pub fn is_dataless(path: &Path) -> bool {
    stat_flags(path).is_some_and(dataless_from_flags)
}

/// The flag test, split out from the platform-specific stat so it is exercised
/// by the test suite on every platform rather than only on the one that can
/// produce the flag.
fn dataless_from_flags(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

#[cfg(target_os = "macos")]
fn stat_flags(path: &Path) -> Option<u32> {
    use std::os::macos::fs::MetadataExt;
    // symlink_metadata, so a link to a placeholder is judged by the link.
    std::fs::symlink_metadata(path).ok().map(|m| m.st_flags())
}

#[cfg(not(target_os = "macos"))]
fn stat_flags(_path: &Path) -> Option<u32> {
    None
}

/// Whether `path`'s name is one no listing, index or watcher event in the
/// notes folder carries: the name `write_atomic` gives the file it writes
/// before renaming it into place, a sync client's in-flight file, an
/// undownloaded placeholder, an editor swap file.
///
/// A walk that runs while a note is being saved would otherwise index the
/// half-written copy and leave a row for a file that no longer exists. The
/// notes watcher and the walk ask the same predicate.
fn is_ignored_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| is_notes_ignored_name(&name.to_string_lossy()))
}

/// Whether the index would hold `path` as a note, size aside.
///
/// The questions [`super::reconcile`]'s walk asks about a file it has reached,
/// in its order and with its answers: a name no listing carries, a placeholder
/// whose kind can only be read off the name, and the kind test itself. Size is
/// left out, because a note over the ceiling is one the index refuses and a
/// link still reaches.
///
/// The pruning half of the walk is [`build_notes_walk`], which every caller of
/// this runs first.
pub(crate) fn indexes_as_note(root: &Path, path: &Path) -> bool {
    if path_has_notes_ignored_name(root, path) {
        return false;
    }
    if is_dataless(path) {
        return has_text_extension(path);
    }
    should_index(path)
}

/// The half of [`indexes_as_note`] that answers from the name alone.
///
/// For a file the walk reached but will not open: a placeholder, whose bytes
/// the read would pull down (ADR-028 §5), and a symlink, whose target is
/// wherever the link points and outside everything this walk was pointed at.
pub(crate) fn names_a_note(root: &Path, path: &Path) -> bool {
    !path_has_notes_ignored_name(root, path) && has_text_extension(path)
}

/// Whether the walk of the notes folder `notes_root` leaves `path` out: a
/// folder on the way to it below `notes_root`, or the file itself, carries a
/// name [`is_notes_ignored_name`] answers for.
///
/// Writ's own writes ask this before they record a file, so a save never gives
/// the index a row the walk would not and the tree does not show. Both paths
/// are compared as [`super::index_key`] spells them, so the names of the
/// folders above `notes_root` are never judged.
pub fn notes_walk_skips(notes_root: &Path, path: &Path) -> bool {
    let root = super::index_key(notes_root);
    let path = super::index_key(path);
    path_has_notes_ignored_name(Path::new(&root), Path::new(&path))
}

/// Whether `path` holds note text worth indexing.
///
/// A known text extension is taken at its word so the common case never opens
/// the file, which means this answers the *kind* question only: callers gate
/// on size themselves. Anything without a known extension is classified, and
/// only a file that opens with the full feature set is indexed.
pub(super) fn should_index(path: &Path) -> bool {
    if is_ignored_path(path) {
        return false;
    }

    if has_text_extension(path) {
        return true;
    }
    matches!(
        classify_path(path).map(|c| c.mode),
        Ok(FileOpenMode::Normal)
    )
}

/// Whether `path` carries one of the extensions Writ indexes without sniffing
/// the file ([`writ_core::notes::extensions::TEXT_EXTENSIONS`]), so the common
/// case never opens a file to decide whether to open it.
///
/// The half of the kind question that answers from the name, which is the only
/// half a file with no local data behind it can be asked. Public so the
/// first launch's read of the notes folder asks the same list the index does,
/// rather than keeping a second one that drifts (ADR-041 §2).
pub fn has_text_extension(path: &Path) -> bool {
    writ_core::notes::extensions::has_text_extension(path)
}

/// Modification time in milliseconds since the Unix epoch, or `0` when the
/// filesystem does not report one. Milliseconds rather than seconds so two
/// edits inside one second are still two different index states.
pub(super) fn mtime_millis(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataless_from_flags_reads_the_sf_dataless_bit() {
        assert!(dataless_from_flags(SF_DATALESS));
        assert!(dataless_from_flags(SF_DATALESS | 0x0000_0002));
    }

    #[test]
    fn dataless_from_flags_ignores_every_other_flag() {
        assert!(!dataless_from_flags(0));
        assert!(!dataless_from_flags(0x0000_0002));
        assert!(!dataless_from_flags(0x0080_0000));
    }

    #[test]
    fn a_file_with_local_data_is_not_dataless_on_any_platform() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("here.md");
        std::fs::write(&path, "local").expect("write");
        assert!(!is_dataless(&path));
    }

    #[test]
    fn should_index_accepts_the_text_extensions() {
        for name in ["a.md", "a.markdown", "a.txt", "a.text", "a.MD"] {
            assert!(
                should_index(Path::new(name)),
                "{name} must be indexed without being opened"
            );
        }
    }

    #[test]
    fn the_notes_walk_reads_build_folders_and_ignores_no_gitignore() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let root = dir.path();
        for rel in [
            "vendor/Acme.md",
            "build/Plan.md",
            "coverage/Report.md",
            "scratch/idea.md",
            "debug.log",
            ".gitignore",
            ".git/HEAD",
            ".obsidian/app.json",
            ".writ-tmp-a1b2c3",
        ] {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, "x").expect("write");
        }
        std::fs::write(root.join(".gitignore"), "scratch/\n*.log\nvendor/\n").expect("write");

        let mut walked: Vec<String> = build_notes_walk(root)
            .build()
            .flatten()
            .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
            .map(|entry| {
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("under root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        walked.sort();
        assert_eq!(
            walked,
            vec![
                ".gitignore",
                "build/Plan.md",
                "coverage/Report.md",
                "debug.log",
                "scratch/idea.md",
                "vendor/Acme.md",
            ]
        );
    }
}
