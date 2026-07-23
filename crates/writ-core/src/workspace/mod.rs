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
];

/// Returns `true` if `name` is in the default ignore set and should be
/// excluded from workspace directory listings.
pub fn is_ignored(name: &str) -> bool {
    DEFAULT_IGNORES.contains(&name)
}

/// The directory names Writ ignores by default, independent of any git ignore
/// configuration. The workspace search walker (in `writ-storage`) feeds these
/// to the `ignore` crate as overrides so the name index and the content grep
/// apply the same union of Writ ignores and gitignore (ADR-026). `.git` is part
/// of the set and is therefore always excluded even though hidden files are
/// otherwise included in search.
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
            WorkspaceEntry {
                name: "zebra.rs".into(),
                path: "/p/zebra.rs".into(),
                is_dir: false,
            },
            WorkspaceEntry {
                name: "alpha".into(),
                path: "/p/alpha".into(),
                is_dir: true,
            },
            WorkspaceEntry {
                name: "bravo.rs".into(),
                path: "/p/bravo.rs".into(),
                is_dir: false,
            },
            WorkspaceEntry {
                name: "zeta".into(),
                path: "/p/zeta".into(),
                is_dir: true,
            },
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
            WorkspaceEntry {
                name: "z.rs".into(),
                path: "/p/z.rs".into(),
                is_dir: false,
            },
            WorkspaceEntry {
                name: "a.rs".into(),
                path: "/p/a.rs".into(),
                is_dir: false,
            },
            WorkspaceEntry {
                name: "m".into(),
                path: "/p/m".into(),
                is_dir: true,
            },
            WorkspaceEntry {
                name: "b".into(),
                path: "/p/b".into(),
                is_dir: true,
            },
        ];
        sort_entries(&mut entries);
        assert_eq!(entries[0].name, "b");
        assert_eq!(entries[1].name, "m");
        assert_eq!(entries[2].name, "a.rs");
        assert_eq!(entries[3].name, "z.rs");
    }
}
