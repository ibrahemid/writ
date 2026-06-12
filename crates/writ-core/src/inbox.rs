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

use crate::workspace::path_has_ignored_component;

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
