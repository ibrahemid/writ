//! Workspace-level policy for Writ.
//!
//! This module defines the domain types and pure policy functions for the
//! workspace file tree: what entries look like, which names are ignored by
//! default, and how entries are ordered for display.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub mod file_search;

/// A single entry (file or directory) in a workspace directory listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    /// File or directory name (not a full path).
    pub name: String,
    /// Absolute path to this entry.
    pub path: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Set when the name says a sync client, or Writ itself, kept a second
    /// copy of another file here. Such a file is listed like any other and
    /// carries the flag with it; hiding it would hide the only copy of text
    /// somebody wrote.
    pub conflict_copy: Option<ConflictCopyKind>,
}

impl WorkspaceEntry {
    /// Describes one listed entry, deciding [`WorkspaceEntry::conflict_copy`]
    /// from the name so every listing flags a copy the same way.
    pub fn new(name: String, path: String, is_dir: bool) -> Self {
        let conflict_copy = if is_dir {
            None
        } else {
            is_conflict_copy(&name)
        };
        Self {
            name,
            path,
            is_dir,
            conflict_copy,
        }
    }
}

/// Who wrote a second copy of a file beside the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictCopyKind {
    /// A sync client kept both sides:
    /// `<stem>.sync-conflict-<date>-<time>-<device>.<ext>`.
    SyncClient,
    /// Writ kept the text a stopped save was carrying:
    /// `<stem> (conflict YYYY-MM-DD HH.MM.SS).<ext>`, the name
    /// `crate::notes::conflict_file_name` writes.
    Writ,
}

const DEFAULT_IGNORES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".DS_Store",
    ".next",
    "build",
    "__pycache__",
    ".cache",
    "coverage",
    "vendor",
    ".obsidian",
    ".trash",
    ".stfolder",
    ".stversions",
];

/// Name patterns for the files a sync client, another editor or Writ's own
/// atomic write leaves in a notes folder. Unlike [`DEFAULT_IGNORES`], which
/// holds whole names, these carry `*` (any run of characters, including none)
/// and `?` (exactly one character).
///
/// Every entry is a file nobody wrote on purpose: a half-downloaded browser
/// file, a placeholder standing in for a file the provider has not fetched, an
/// editor swap or backup file, or the temp file `write_atomic` renames into
/// place. A conflict copy is deliberately not here: it holds text somebody
/// wrote, so it is listed and flagged instead ([`is_conflict_copy`]).
const IGNORE_GLOBS: &[&str] = &[
    // Obsidian's key map, which sits beside the notes rather than in
    // `.obsidian`.
    ".obsidian.vimrc",
    // Syncthing's in-flight temp files, both spellings it has shipped.
    ".syncthing.*.tmp",
    "~syncthing~*.tmp",
    // An iCloud placeholder: the name of a file that is not downloaded.
    ".*.icloud",
    // A download still arriving.
    "*.crdownload",
    "*.partial",
    // Dropbox's own bookkeeping files (`.dropbox`, `.dropbox.cache`).
    ".dropbox*",
    // vim swap files: `.note.md.swp`, `.swo`, `.swn`.
    ".*.sw?",
    // Emacs and friends: a backup of the file next to it.
    "*~",
    // The temp file `write_atomic` creates beside its target, in both the
    // prefix form `NamedTempFile` produces and the suffix form.
    ".tmp*",
    "*.tmp",
];

/// Returns `true` if `name` is in the default ignore set and should be
/// excluded from workspace directory listings.
pub fn is_ignored(name: &str) -> bool {
    DEFAULT_IGNORES.contains(&name)
}

/// Returns `true` when a file or directory called `name` is one Writ never
/// lists, indexes or reports a change for: a [`DEFAULT_IGNORES`] name, or a
/// name matching [`IGNORE_GLOBS`].
///
/// This is the one answer the tree listing, the notes index walk and the notes
/// watcher share, so a file that is invisible in one is invisible in all three
/// (ADR-028 section 6).
///
/// Matching folds ASCII case, because the filesystems Writ runs on mostly do
/// and a `.CRDOWNLOAD` is the same file as a `.crdownload`.
pub fn is_ignored_name(name: &str) -> bool {
    if is_ignored(name) {
        return true;
    }
    IGNORE_GLOBS
        .iter()
        .any(|pattern| glob_matches(pattern, name))
}

/// Matches `name` against a `*`/`?` pattern, folding ASCII case.
///
/// `*` matches any run of characters including none, `?` exactly one. The walk
/// is the standard backtracking one: on a mismatch it returns to the last `*`
/// and lets it consume one more character, which is linear in practice for the
/// short patterns in [`IGNORE_GLOBS`] and needs no allocation beyond the two
/// character vectors.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();

    let (mut p, mut n) = (0usize, 0usize);
    // Where the last `*` sits, and how much of `name` it has consumed.
    let mut star: Option<usize> = None;
    let mut consumed = 0usize;

    while n < name.len() {
        let matched =
            p < pattern.len() && (pattern[p] == '?' || pattern[p].eq_ignore_ascii_case(&name[n]));
        if matched {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            consumed = n;
            p += 1;
        } else if let Some(star) = star {
            consumed += 1;
            n = consumed;
            p = star + 1;
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|c| *c == '*')
}

/// Names the file `name` is a second copy of, or `None` when it is not a copy.
///
/// Two producers are recognised: a sync client's
/// `<stem>.sync-conflict-<date>-<time>-<device>.<ext>`, and Writ's own
/// `<stem> (conflict YYYY-MM-DD HH.MM.SS).<ext>` from
/// [`crate::notes::conflict_file_name`]. Both are matched on the timestamp that
/// follows the marker, so a note somebody named `merge (conflict notes).md`
/// stays an ordinary note.
pub fn is_conflict_copy(name: &str) -> Option<ConflictCopyKind> {
    if let Some(rest) = name.split_once(".sync-conflict-").map(|(_, rest)| rest) {
        if has_sync_client_stamp(rest) {
            return Some(ConflictCopyKind::SyncClient);
        }
    }
    if let Some((_, rest)) = name.rsplit_once(" (conflict ") {
        if let Some((stamp, after)) = rest.split_once(')') {
            if is_writ_stamp(stamp) && (after.is_empty() || after.starts_with('.')) {
                return Some(ConflictCopyKind::Writ);
            }
        }
    }
    None
}

/// `YYYYMMDD-HHMMSS-` at the front of what follows a sync client's marker.
fn has_sync_client_stamp(rest: &str) -> bool {
    let mut chars = rest.chars();
    (0..8).all(|_| chars.next().is_some_and(|c| c.is_ascii_digit()))
        && chars.next() == Some('-')
        && (0..6).all(|_| chars.next().is_some_and(|c| c.is_ascii_digit()))
        && chars.next() == Some('-')
}

/// `YYYY-MM-DD HH.MM.SS`, the stamp [`crate::notes::conflict_file_name`] writes.
fn is_writ_stamp(stamp: &str) -> bool {
    const SHAPE: &str = "dddd-dd-dd dd.dd.dd";
    stamp.len() == SHAPE.len()
        && stamp
            .chars()
            .zip(SHAPE.chars())
            .all(|(c, shape)| match shape {
                'd' => c.is_ascii_digit(),
                other => c == other,
            })
}

/// The directory names Writ ignores by default, independent of any git ignore
/// configuration. The workspace search walker (in `writ-storage`) hands these
/// to a `filter_entry` closure on the walk builder, so the name index and the
/// content grep apply the same union of Writ ignores and gitignore (ADR-026).
/// `.git` is part of the set and is therefore always excluded even though
/// hidden files are otherwise included in search.
///
/// The set also carries the folders a note folder imported from another tool
/// or synced by another client leaves behind — `.obsidian`, `.trash`,
/// `.stfolder`, `.stversions` — so the tree, the walk and the watcher all skip
/// them from the one constant.
pub fn default_ignored_dirs() -> &'static [&'static str] {
    DEFAULT_IGNORES
}

/// Returns `true` when any component of `path` below `root` matches the
/// default ignore set, so watcher events from ignored directories (for
/// example `node_modules` churn) never surface as workspace changes.
pub fn path_has_ignored_component(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    relative
        .components()
        .any(|c| is_ignored(&c.as_os_str().to_string_lossy()))
}

/// Returns `true` when any component of `path` below `root` is a name
/// [`is_ignored_name`] answers for, which is the notes folder's question:
/// a file inside a folder a sync client keeps for itself (`.dropbox.cache`) is
/// as invisible as the folder is.
///
/// [`path_has_ignored_component`] answers the narrower question and stays as
/// it is: the workspace grep, the inbox and the workspace watcher still walk
/// into a directory named `build~`.
pub fn path_has_ignored_name(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    relative
        .components()
        .any(|c| is_ignored_name(&c.as_os_str().to_string_lossy()))
}

/// Sorts `entries` in-place: directories first, then files, each group
/// ordered case-insensitively by name.
pub fn sort_entries(entries: &mut [WorkspaceEntry]) {
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_names_are_rejected() {
        for name in &[
            ".git",
            "node_modules",
            "target",
            "dist",
            ".DS_Store",
            ".next",
            "build",
            "__pycache__",
            ".cache",
            "coverage",
            "vendor",
            ".obsidian",
            ".trash",
            ".stfolder",
            ".stversions",
        ] {
            assert!(is_ignored(name), "{name} should be ignored");
        }
    }

    #[test]
    fn default_ignored_dirs_carries_the_writ_side_of_the_union() {
        let dirs = default_ignored_dirs();
        // `.git` is always excluded; the common heavy build/dep dirs too.
        assert!(dirs.contains(&".git"));
        assert!(dirs.contains(&"node_modules"));
        assert!(dirs.contains(&"target"));
        // Dotfiles are included in search (ADR-026): a plain `.env` is not a
        // Writ default ignore, so only gitignore can exclude it. This is the
        // seam where the union's git half does the work.
        assert!(!dirs.contains(&".env"));
        assert!(!is_ignored(".env"));
        assert!(!is_ignored(".github"));
    }

    #[test]
    fn obsidian_folder_is_ignored_so_an_imported_note_folder_lists_only_notes() {
        // A folder imported from another editor carries its settings folder.
        // One constant feeds the tree, the search walk and the watcher, so
        // ignoring the name here ignores it everywhere.
        assert!(is_ignored(".obsidian"));
        assert!(default_ignored_dirs().contains(&".obsidian"));
        assert!(path_has_ignored_component(
            Path::new("/notes"),
            Path::new("/notes/.obsidian/workspace.json")
        ));
        assert!(!is_ignored("notes.md"));
    }

    #[test]
    fn sync_and_editor_leftovers_are_ignored_by_name() {
        for name in &[
            ".obsidian.vimrc",
            ".syncthing.note.md.tmp",
            "~syncthing~note.md.tmp",
            ".note.md.icloud",
            "report.pdf.crdownload",
            "report.pdf.partial",
            ".dropbox",
            ".dropbox.cache",
            ".note.md.swp",
            ".note.md.swo",
            "note.md~",
            ".tmpA1b2C3",
            "note.md.tmp",
        ] {
            assert!(is_ignored_name(name), "{name} should be ignored");
        }
    }

    #[test]
    fn a_file_inside_a_sync_clients_own_folder_is_ignored_by_path() {
        let root = Path::new("/notes");
        assert!(path_has_ignored_name(
            root,
            Path::new("/notes/.dropbox.cache/copy.md")
        ));
        assert!(path_has_ignored_name(
            root,
            Path::new("/notes/.obsidian/workspace.json")
        ));
        assert!(!path_has_ignored_name(root, Path::new("/notes/day/one.md")));
        assert!(!path_has_ignored_name(
            Path::new("/notes"),
            Path::new("/elsewhere/.dropbox.cache/copy.md")
        ));
    }

    #[test]
    fn ignore_globs_fold_case() {
        assert!(is_ignored_name("REPORT.PDF.CRDOWNLOAD"));
        assert!(is_ignored_name("Note.MD.Tmp"));
    }

    #[test]
    fn notes_are_not_caught_by_the_glob_set() {
        for name in &[
            "note.md",
            "notes.txt",
            "swap.md",
            "icloud.md",
            "partially done.md",
            "dropbox notes.md",
            "tmp.md",
            ".obsidian.md",
            "~tilde.md",
        ] {
            assert!(!is_ignored_name(name), "{name} should be listed");
        }
    }

    #[test]
    fn the_whole_name_ignore_set_still_answers_through_the_glob_set() {
        assert!(is_ignored_name(".obsidian"));
        assert!(is_ignored_name("node_modules"));
    }

    #[test]
    fn writ_temp_file_names_are_ignored_in_both_spellings() {
        // `write_atomic` persists through a `NamedTempFile` created beside its
        // target, so the prefix form is the one every internal save produces;
        // the suffix form is what other tools leave. The watcher and the index
        // walk both go through this predicate, so both forms have to answer
        // here (ADR-028 section 6).
        assert!(is_ignored_name(".tmpABC"));
        assert!(is_ignored_name(".tmp"));
        assert!(is_ignored_name("foo.tmp"));
    }

    #[test]
    fn glob_matcher_handles_stars_and_single_character_wildcards() {
        assert!(glob_matches("*", ""));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("a*b*c", "abc"));
        assert!(glob_matches("a*b*c", "axxbyyc"));
        assert!(!glob_matches("a*b*c", "axxbyy"));
        assert!(glob_matches(".*.sw?", ".a.swp"));
        assert!(!glob_matches(".*.sw?", ".a.sw"));
        assert!(!glob_matches(".*.sw?", ".a.swap"));
        assert!(glob_matches("exact", "exact"));
        assert!(!glob_matches("exact", "exacts"));
        // A multi-byte name is walked by character, not by byte.
        assert!(glob_matches("*~", "ملاحظة~"));
        assert!(!glob_matches("*~", "ملاحظة"));
    }

    #[test]
    fn conflict_copies_are_recognised_by_producer() {
        assert_eq!(
            is_conflict_copy("note.sync-conflict-20260822-120000-ABCD.md"),
            Some(ConflictCopyKind::SyncClient)
        );
        assert_eq!(
            is_conflict_copy("note (conflict 2026-08-22 12.00.00).md"),
            Some(ConflictCopyKind::Writ)
        );
        assert_eq!(
            is_conflict_copy("note (conflict 2026-08-22 12.00.00)"),
            Some(ConflictCopyKind::Writ)
        );
    }

    #[test]
    fn the_name_writ_writes_is_the_name_this_recognises() {
        // The producer and the predicate are one pair: if `conflict_file_name`
        // changes shape, this fails rather than silently listing Writ's own
        // copies unflagged.
        let name = crate::notes::conflict_file_name("note", "md", chrono::Utc::now());
        assert_eq!(is_conflict_copy(&name), Some(ConflictCopyKind::Writ));
        assert!(!is_ignored_name(&name));
    }

    #[test]
    fn a_note_that_only_reads_like_a_copy_is_an_ordinary_note() {
        assert_eq!(is_conflict_copy("note.md"), None);
        assert_eq!(is_conflict_copy("merge (conflict notes).md"), None);
        assert_eq!(is_conflict_copy("note.sync-conflict-notes.md"), None);
        assert_eq!(is_conflict_copy("note (conflict 2026-08-22).md"), None);
    }

    #[test]
    fn a_conflict_copy_is_listed_rather_than_ignored() {
        // The copy is the only place its text lives, so hiding it loses the
        // text. It is listed with the flag on it instead.
        for name in &[
            "note.sync-conflict-20260822-120000-ABCD.md",
            "note (conflict 2026-08-22 12.00.00).md",
        ] {
            assert!(!is_ignored_name(name), "{name} should be listed");
            assert!(is_conflict_copy(name).is_some());
        }
    }

    #[test]
    fn entries_carry_the_conflict_flag_and_directories_never_do() {
        let file = WorkspaceEntry::new(
            "note (conflict 2026-08-22 12.00.00).md".into(),
            "/notes/note (conflict 2026-08-22 12.00.00).md".into(),
            false,
        );
        assert_eq!(file.conflict_copy, Some(ConflictCopyKind::Writ));

        let plain = WorkspaceEntry::new("note.md".into(), "/notes/note.md".into(), false);
        assert_eq!(plain.conflict_copy, None);

        let dir = WorkspaceEntry::new(
            "note (conflict 2026-08-22 12.00.00).md".into(),
            "/notes/folder".into(),
            true,
        );
        assert_eq!(dir.conflict_copy, None);
    }

    #[test]
    fn conflict_kind_serialises_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&ConflictCopyKind::SyncClient).unwrap(),
            "\"sync_client\""
        );
        assert_eq!(
            serde_json::to_string(&ConflictCopyKind::Writ).unwrap(),
            "\"writ\""
        );
    }

    #[test]
    fn normal_names_pass_ignore_filter() {
        for name in &["src", "main.rs", "README.md", "Cargo.toml", "lib"] {
            assert!(!is_ignored(name), "{name} should not be ignored");
        }
    }

    #[test]
    fn ignored_component_anywhere_below_root_is_rejected() {
        let root = Path::new("/ws");
        assert!(path_has_ignored_component(
            root,
            Path::new("/ws/node_modules/pkg/index.js")
        ));
        assert!(path_has_ignored_component(
            root,
            Path::new("/ws/app/target/debug/out")
        ));
        assert!(path_has_ignored_component(root, Path::new("/ws/.git/HEAD")));
    }

    #[test]
    fn normal_paths_below_root_pass() {
        let root = Path::new("/ws");
        assert!(!path_has_ignored_component(
            root,
            Path::new("/ws/src/main.rs")
        ));
        assert!(!path_has_ignored_component(root, Path::new("/ws/notes.md")));
    }

    #[test]
    fn path_outside_root_is_not_flagged() {
        assert!(!path_has_ignored_component(
            Path::new("/ws"),
            Path::new("/elsewhere/node_modules/x")
        ));
    }

    #[test]
    fn ignored_name_in_root_prefix_does_not_flag() {
        let root = Path::new("/home/u/node_modules/myproj");
        assert!(!path_has_ignored_component(
            root,
            Path::new("/home/u/node_modules/myproj/src/a.rs")
        ));
    }

    #[test]
    fn sort_dirs_before_files() {
        let mut entries = vec![
            WorkspaceEntry::new("zebra.rs".into(), "/p/zebra.rs".into(), false),
            WorkspaceEntry::new("alpha".into(), "/p/alpha".into(), true),
            WorkspaceEntry::new("bravo.rs".into(), "/p/bravo.rs".into(), false),
            WorkspaceEntry::new("zeta".into(), "/p/zeta".into(), true),
        ];
        sort_entries(&mut entries);
        assert!(entries[0].is_dir);
        assert!(entries[1].is_dir);
        assert!(!entries[2].is_dir);
        assert!(!entries[3].is_dir);
    }

    #[test]
    fn sort_alphabetical_within_kind() {
        let mut entries = vec![
            WorkspaceEntry::new("z.rs".into(), "/p/z.rs".into(), false),
            WorkspaceEntry::new("a.rs".into(), "/p/a.rs".into(), false),
            WorkspaceEntry::new("m".into(), "/p/m".into(), true),
            WorkspaceEntry::new("b".into(), "/p/b".into(), true),
        ];
        sort_entries(&mut entries);
        assert_eq!(entries[0].name, "b");
        assert_eq!(entries[1].name, "m");
        assert_eq!(entries[2].name, "a.rs");
        assert_eq!(entries[3].name, "z.rs");
    }
}
