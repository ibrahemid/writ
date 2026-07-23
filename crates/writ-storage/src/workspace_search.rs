//! Workspace file-name index walk and the shared search ignore policy.
//!
//! The bounded walk in [`walk_index`] enumerates every file under the workspace
//! root that survives the union ignore policy (ADR-026): Writ's default ignores
//! (`node_modules`, `target`, `.git`, and the rest) plus the git ignore sources
//! (`.gitignore`, `.ignore`, global gitignore). Hidden files are included; only
//! `.git/` and the Writ defaults are pruned. The same [`build_walk`] is the base
//! for the content grep, so name search and content search agree on which files
//! exist.
//!
//! [`is_path_indexed`] answers the single-path question the watcher patch path
//! needs: given one changed path, should it be in the name index? It applies the
//! Writ defaults, the root `.gitignore`/`.ignore`, and the global gitignore. It
//! does not read `.gitignore`s nested in subdirectories (the `ignore` crate has
//! no single-path query over the full nested stack); a full rebuild is the
//! convergence backstop for that gap.

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;

/// Result of a bounded workspace file walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexWalk {
    /// Workspace-relative file paths (forward-slash separated).
    pub paths: Vec<String>,
    /// `true` when the walk stopped at `cap` and more files exist.
    pub truncated: bool,
}

/// Builds the shared search walker for `root`: hidden files included, symlinks
/// never followed, git ignore sources honored even outside a git repo, and
/// Writ's default-ignored directories pruned. Used by both the name index walk
/// and the content grep so the two apply one ignore policy.
pub fn build_walk(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true);

    let ignores = writ_core::workspace::default_ignored_dirs();
    builder.filter_entry(move |entry| {
        let name = entry.file_name().to_string_lossy();
        !ignores.iter().any(|ig| *ig == name)
    });
    builder
}

/// Normalizes `path` to a workspace-relative, forward-slash string, or `None`
/// when it is not under `root` or is the root itself.
fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    let mut s = rel.to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR == '\\' {
        s = s.replace('\\', "/");
    }
    Some(s)
}

/// Walks `root` and collects up to `cap` workspace-relative file paths under the
/// union ignore policy. On overflow the walk stops and `truncated` is `true`.
pub fn walk_index(root: &Path, cap: usize) -> IndexWalk {
    let mut paths = Vec::new();
    let mut truncated = false;

    for result in build_walk(root).build() {
        let Ok(entry) = result else { continue };
        match entry.file_type() {
            Some(ft) if ft.is_file() => {}
            _ => continue,
        }
        let Some(rel) = relative_path(root, entry.path()) else {
            continue;
        };
        if paths.len() >= cap {
            truncated = true;
            break;
        }
        paths.push(rel);
    }

    IndexWalk { paths, truncated }
}

/// Builds a gitignore matcher from `root`'s own `.gitignore` and `.ignore`.
fn root_gitignore(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    builder.add(root.join(".gitignore"));
    builder.add(root.join(".ignore"));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// Returns `true` when `path` belongs in the name index: it is under `root`, is
/// not a Writ default-ignored component, and is not matched by the root
/// gitignore or the global gitignore. Nested subdirectory `.gitignore`s are not
/// consulted (see the module docs); a full rebuild reconciles that gap.
pub fn is_path_indexed(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    if rel.as_os_str().is_empty() {
        return false;
    }
    if writ_core::workspace::path_has_ignored_component(root, path) {
        return false;
    }

    // Match on the workspace-relative path: an absolute path outside a matcher's
    // own root makes `matched_path_or_any_parents` panic, and the global
    // matcher's root is the home directory, not the workspace.
    let is_dir = path.is_dir();
    if root_gitignore(root)
        .matched_path_or_any_parents(rel, is_dir)
        .is_ignore()
    {
        return false;
    }
    let (global, _) = Gitignore::global();
    if global.matched_path_or_any_parents(rel, is_dir).is_ignore() {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn walk_lists_files_relative_to_root() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "src/main.rs", "fn main() {}");
        write(dir.path(), "README.md", "# hi");

        let walk = walk_index(dir.path(), 1000);
        assert!(walk.paths.contains(&"src/main.rs".to_string()));
        assert!(walk.paths.contains(&"README.md".to_string()));
        assert!(!walk.truncated);
    }

    #[test]
    fn walk_prunes_writ_default_ignores() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "node_modules/pkg/index.js", "x");
        write(dir.path(), "target/debug/app", "x");
        write(dir.path(), ".git/HEAD", "ref: x");
        write(dir.path(), "src/lib.rs", "x");

        let walk = walk_index(dir.path(), 1000);
        assert_eq!(walk.paths, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn walk_honors_gitignore() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), ".gitignore", "secret.txt\nbuild-out/\n");
        write(dir.path(), "secret.txt", "shh");
        write(dir.path(), "build-out/artifact.bin", "x");
        write(dir.path(), "keep.rs", "x");

        let walk = walk_index(dir.path(), 1000);
        assert!(walk.paths.contains(&"keep.rs".to_string()));
        // .gitignore itself is a dotfile and is included (dotfiles are searched).
        assert!(walk.paths.contains(&".gitignore".to_string()));
        assert!(!walk.paths.contains(&"secret.txt".to_string()));
        assert!(!walk.paths.iter().any(|p| p.starts_with("build-out/")));
    }

    #[test]
    fn walk_includes_dotfiles_except_git() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), ".env", "TOKEN=1");
        write(dir.path(), ".github/workflows/ci.yml", "on: push");
        write(dir.path(), ".git/config", "[core]");

        let walk = walk_index(dir.path(), 1000);
        assert!(walk.paths.contains(&".env".to_string()));
        assert!(walk.paths.contains(&".github/workflows/ci.yml".to_string()));
        assert!(!walk.paths.iter().any(|p| p.starts_with(".git/")));
    }

    #[test]
    fn walk_marks_truncated_at_cap() {
        let dir = TempDir::new().unwrap();
        for i in 0..10 {
            write(dir.path(), &format!("f{i}.txt"), "x");
        }
        let walk = walk_index(dir.path(), 4);
        assert_eq!(walk.paths.len(), 4);
        assert!(walk.truncated);
    }

    #[test]
    fn is_path_indexed_matches_the_walk_policy() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), ".gitignore", "secret.txt\n");
        write(dir.path(), "keep.rs", "x");
        write(dir.path(), "secret.txt", "x");
        write(dir.path(), "node_modules/pkg/a.js", "x");
        write(dir.path(), ".env", "x");

        let root = dir.path();
        assert!(is_path_indexed(root, &root.join("keep.rs")));
        assert!(is_path_indexed(root, &root.join(".env")));
        assert!(!is_path_indexed(root, &root.join("secret.txt")));
        assert!(!is_path_indexed(root, &root.join("node_modules/pkg/a.js")));
        // A path outside the root is never indexed.
        let other = TempDir::new().unwrap();
        assert!(!is_path_indexed(root, &other.path().join("x.rs")));
    }
}
