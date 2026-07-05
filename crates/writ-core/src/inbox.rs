//! Watch-inbox policy for Writ (ADR-018, arrival rule revised by ADR-024).
//!
//! Pure rules deciding whether a file that appeared inside the watched
//! inbox folder should auto-open. The adapter supplies the filesystem
//! facts (the snapshot of paths that already existed when watching
//! began); this module owns the decision: containment under the inbox
//! root, the shared default ignore set, and the not-in-snapshot rule
//! that keeps pre-existing backlogs and mere modifications from opening
//! tabs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workspace::path_has_ignored_component;

/// A regular file listed in the watched inbox folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxFile {
    /// File name (not a full path).
    pub name: String,
    /// Absolute path to the file, used to open it.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
}

/// Sorts inbox files in-place, case-insensitively by name.
pub fn sort_inbox_files(files: &mut [InboxFile]) {
    files.sort_by_key(|f| f.name.to_lowercase());
}

/// Returns `true` when a file at `path` should auto-open from the inbox.
///
/// Qualifying files:
///
/// - sit inside `root`,
/// - have no default-ignored component (`node_modules`, `.git`, …)
///   between `root` and the file, and
/// - are absent from `preexisting`, the snapshot of paths taken when
///   watching began, so pre-existing files (including pre-existing files
///   later modified) never auto-open.
///
/// Snapshot membership replaces the original creation-timestamp
/// comparison: filesystem birth times carry coarse granularity on some
/// systems, which silently suppressed genuinely new files created within
/// the same clock tick as watch start (ADR-024).
pub fn qualifies_for_auto_open(root: &Path, path: &Path, preexisting: &HashSet<PathBuf>) -> bool {
    if !path.starts_with(root) {
        return false;
    }
    if path_has_ignored_component(root, path) {
        return false;
    }
    !preexisting.contains(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> InboxFile {
        InboxFile {
            name: name.to_string(),
            path: format!("/inbox/{name}"),
            size_bytes: 0,
        }
    }

    fn snapshot(paths: &[&str]) -> HashSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn sort_inbox_files_orders_case_insensitively_by_name() {
        let mut files = vec![file("Zebra.md"), file("apple.md"), file("Banana.md")];
        sort_inbox_files(&mut files);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["apple.md", "Banana.md", "Zebra.md"]);
    }

    #[test]
    fn file_absent_from_snapshot_qualifies() {
        assert!(qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/report.md"),
            &snapshot(&["/inbox/old-report.md"]),
        ));
    }

    #[test]
    fn file_qualifies_against_an_empty_snapshot() {
        assert!(qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/report.md"),
            &snapshot(&[]),
        ));
    }

    #[test]
    fn backlog_file_in_snapshot_is_excluded() {
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/old-report.md"),
            &snapshot(&["/inbox/old-report.md"]),
        ));
    }

    #[test]
    fn path_outside_root_is_excluded() {
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/elsewhere/report.md"),
            &snapshot(&[]),
        ));
    }

    #[test]
    fn path_under_ignored_component_is_excluded() {
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/node_modules/pkg/readme.md"),
            &snapshot(&[]),
        ));
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/.git/COMMIT_EDITMSG"),
            &snapshot(&[]),
        ));
    }
}
