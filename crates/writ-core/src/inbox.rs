//! Watch-inbox policy for Writ (ADR-018).
//!
//! Pure rules deciding whether a file that appeared inside the watched
//! inbox folder should auto-open. The adapter supplies the filesystem
//! facts (creation timestamp, watch-start instant); this module owns the
//! decision: containment under the inbox root, the shared default ignore
//! set, and the created-after-watch-start rule that keeps pre-existing
//! backlogs and mere modifications from opening tabs.

use std::path::Path;
use std::time::SystemTime;

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
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
}

/// Returns `true` when a file at `path` should auto-open from the inbox.
///
/// Qualifying files:
///
/// - sit inside `root`,
/// - have no default-ignored component (`node_modules`, `.git`, …)
///   between `root` and the file, and
/// - were created at or after `watch_start`, so pre-existing files —
///   including pre-existing files later modified — never auto-open.
pub fn qualifies_for_auto_open(
    root: &Path,
    path: &Path,
    created: SystemTime,
    watch_start: SystemTime,
) -> bool {
    if !path.starts_with(root) {
        return false;
    }
    if path_has_ignored_component(root, path) {
        return false;
    }
    created >= watch_start
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn file(name: &str) -> InboxFile {
        InboxFile {
            name: name.to_string(),
            path: format!("/inbox/{name}"),
            size_bytes: 0,
        }
    }

    #[test]
    fn sort_inbox_files_orders_case_insensitively_by_name() {
        let mut files = vec![file("Zebra.md"), file("apple.md"), file("Banana.md")];
        sort_inbox_files(&mut files);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["apple.md", "Banana.md", "Zebra.md"]);
    }

    #[test]
    fn file_created_after_watch_start_qualifies() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let created = start + Duration::from_secs(1);
        assert!(qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/report.md"),
            created,
            start,
        ));
    }

    #[test]
    fn file_created_exactly_at_watch_start_qualifies() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/report.md"),
            start,
            start,
        ));
    }

    #[test]
    fn backlog_file_created_before_watch_start_is_excluded() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let created = start - Duration::from_secs(1);
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/old-report.md"),
            created,
            start,
        ));
    }

    #[test]
    fn path_outside_root_is_excluded() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/elsewhere/report.md"),
            start + Duration::from_secs(1),
            start,
        ));
    }

    #[test]
    fn path_under_ignored_component_is_excluded() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/node_modules/pkg/readme.md"),
            start + Duration::from_secs(1),
            start,
        ));
        assert!(!qualifies_for_auto_open(
            Path::new("/inbox"),
            Path::new("/inbox/.git/COMMIT_EDITMSG"),
            start + Duration::from_secs(1),
            start,
        ));
    }
}
