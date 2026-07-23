//! In-memory workspace file-name index (ADR-026, C2).
//!
//! Holds one entry per workspace file: a relative-path `String` plus a `u32`
//! offset to its file-name segment, so a 200k-file index costs one string per
//! file rather than two. The index is built by a bounded walk (in
//! `writ-storage`) on workspace open and maintained incrementally from the
//! recursive workspace watcher: a changed path that still exists and passes the
//! union ignore policy is upserted, a gone path is removed.
//!
//! ## Build-vs-watcher race
//!
//! The initial walk (up to [`MAX_INDEXED_FILES`] files) runs on a background
//! thread and swaps its result into the shared index under the write lock. A
//! watcher event that lands while a build is in flight cannot be applied as a
//! patch (the entries are about to be replaced), so it sets `rebuild_needed`
//! instead; the build thread checks that flag immediately after the swap and
//! re-runs, so an update that arrived mid-walk is never discarded. A
//! `build_epoch` supersedes any in-flight build when the root changes, so a
//! stale walk can never clobber a newer one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use writ_core::workspace::file_search::{rank_file_hits, FileHit};
use writ_storage::workspace_search::{is_path_indexed, walk_index, IndexWalk};

/// Upper bound on indexed files; past it the index is marked truncated.
pub const MAX_INDEXED_FILES: usize = 200_000;

/// Shared handle to the workspace index, cloned into the watcher subscriber and
/// the build thread.
pub type SharedIndex = Arc<RwLock<WorkspaceIndex>>;

/// One indexed file: its workspace-relative path and the byte offset of the
/// file-name segment within that path (`&path[name_offset..]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    path: String,
    name_offset: u32,
}

impl IndexedFile {
    fn new(path: String) -> Self {
        let name_offset = path.rfind(['/', '\\']).map(|i| i + 1).unwrap_or(0) as u32;
        Self { path, name_offset }
    }

    /// The file-name segment.
    pub fn name(&self) -> &str {
        &self.path[self.name_offset as usize..]
    }

    /// The workspace-relative path.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Truncation-aware status of the index for the UI notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct IndexStatus {
    /// Number of indexed files.
    pub file_count: usize,
    /// `true` when the file count hit the cap and more files exist.
    pub truncated: bool,
    /// `true` when a workspace folder is open.
    pub has_workspace: bool,
}

/// The workspace file-name index.
pub struct WorkspaceIndex {
    root: Option<PathBuf>,
    entries: Vec<IndexedFile>,
    /// Path to position in `entries`, for O(1) patch lookups.
    positions: HashMap<String, usize>,
    truncated: bool,
    /// Bumped on every successful build swap; the file-name search uses it, and
    /// tests observe rebuilds through it.
    generation: u64,
    /// A background build is in flight.
    building: bool,
    /// A watcher event arrived mid-build and must force a re-run after the swap.
    rebuild_needed: bool,
    /// Identifies the current build; a swap from a stale epoch is ignored.
    build_epoch: u64,
}

impl WorkspaceIndex {
    /// Creates an empty index for `root` (which may be `None`).
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            entries: Vec::new(),
            positions: HashMap::new(),
            truncated: false,
            generation: 0,
            building: false,
            rebuild_needed: false,
            build_epoch: 0,
        }
    }

    /// Replaces the workspace root, clearing all entries and superseding any
    /// in-flight build.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        self.root = root;
        self.entries.clear();
        self.positions.clear();
        self.truncated = false;
        self.building = false;
        self.rebuild_needed = false;
        self.build_epoch += 1;
        self.generation += 1;
    }

    /// Reports the index status.
    pub fn status(&self) -> IndexStatus {
        IndexStatus {
            file_count: self.entries.len(),
            truncated: self.truncated,
            has_workspace: self.root.is_some(),
        }
    }

    /// Ranks the index against `query`, returning up to `limit` hits.
    pub fn search(&self, query: &str, limit: usize) -> Vec<FileHit> {
        rank_file_hits(
            query,
            self.entries.iter().map(|e| (e.path(), e.name())),
            limit,
        )
    }

    /// Current build generation (test/observability hook).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Begins a build: returns `(root, epoch)` when the caller should walk, or
    /// `None` when there is no root or a build is already running (in which case
    /// `rebuild_needed` is set so the running build re-runs).
    fn begin_build(&mut self) -> Option<(PathBuf, u64)> {
        let root = self.root.clone()?;
        if self.building {
            self.rebuild_needed = true;
            return None;
        }
        self.building = true;
        self.rebuild_needed = false;
        self.build_epoch += 1;
        Some((root, self.build_epoch))
    }

    /// Applies a completed walk if `epoch` is still current. Returns `true` when
    /// a mid-build event requires another walk (the build stays armed); returns
    /// `false` when the build is done or was superseded.
    fn apply_build(&mut self, epoch: u64, walk: IndexWalk) -> bool {
        if epoch != self.build_epoch {
            // Superseded by a newer build or a root change; drop this result.
            return false;
        }
        self.entries = walk.paths.into_iter().map(IndexedFile::new).collect();
        self.positions = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.clone(), i))
            .collect();
        self.truncated = walk.truncated;
        self.generation += 1;

        if self.rebuild_needed {
            self.rebuild_needed = false;
            true
        } else {
            self.building = false;
            false
        }
    }

    /// Adds `rel` to the index if absent (respecting the cap).
    fn upsert(&mut self, rel: String) {
        if self.positions.contains_key(&rel) {
            return;
        }
        if self.entries.len() >= MAX_INDEXED_FILES {
            self.truncated = true;
            return;
        }
        let idx = self.entries.len();
        self.positions.insert(rel.clone(), idx);
        self.entries.push(IndexedFile::new(rel));
    }

    /// Removes `rel` from the index if present.
    fn remove(&mut self, rel: &str) {
        let Some(idx) = self.positions.remove(rel) else {
            return;
        };
        let last = self.entries.len() - 1;
        self.entries.swap_remove(idx);
        if idx != last {
            // The element that was at `last` now sits at `idx`; fix its position.
            let moved = self.entries[idx].path.clone();
            self.positions.insert(moved, idx);
        }
    }
}

/// Locks the shared index for writing, recovering from poisoning.
fn write(index: &SharedIndex) -> std::sync::RwLockWriteGuard<'_, WorkspaceIndex> {
    index.write().unwrap_or_else(|e| {
        tracing::error!(
            location = "workspace_index::write",
            "recovered poisoned RwLock"
        );
        e.into_inner()
    })
}

/// Normalizes an absolute workspace path to a relative, forward-slash string.
fn relative(root: &Path, path: &Path) -> Option<String> {
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

/// Rebuilds `index` synchronously from a fresh walk, blocking until the swap is
/// done. A no-op when there is no workspace root or a build is already running
/// (in which case a re-run is armed for the running build). Re-walks once more
/// when an event arrived mid-build.
pub fn rebuild_blocking(index: &SharedIndex) {
    let Some((root, epoch)) = write(index).begin_build() else {
        return;
    };
    loop {
        let walk = walk_index(&root, MAX_INDEXED_FILES);
        if !write(index).apply_build(epoch, walk) {
            break;
        }
    }
}

/// Kicks off a background rebuild of `index`. Safe to call repeatedly; a build
/// already in flight only arms a re-run.
pub fn spawn_rebuild(index: SharedIndex) {
    std::thread::spawn(move || rebuild_blocking(&index));
}

/// Sets the workspace root and rebuilds the index from scratch.
pub fn set_root_and_rebuild(index: &SharedIndex, root: Option<PathBuf>) {
    let has_root = root.is_some();
    write(index).set_root(root);
    if has_root {
        spawn_rebuild(index.clone());
    }
}

/// Handles one `WorkspaceChanged` event against the index. During a build it
/// only arms a rebuild; otherwise it patches the single path in place.
pub fn on_workspace_changed(index: &SharedIndex, path: &str, removed: bool) {
    let mut idx = write(index);
    let Some(root) = idx.root.clone() else {
        return;
    };
    if idx.building {
        idx.rebuild_needed = true;
        return;
    }
    let abs = Path::new(path);
    let Some(rel) = relative(&root, abs) else {
        return;
    };
    if removed || !abs.exists() {
        idx.remove(&rel);
    } else if abs.is_file() && is_path_indexed(&root, abs) {
        // The watcher fires for directory creation too and does not distinguish
        // it from a file; the index holds files only, matching the build walk.
        idx.upsert(rel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn paths(idx: &WorkspaceIndex) -> Vec<String> {
        let mut p: Vec<String> = idx.entries.iter().map(|e| e.path.clone()).collect();
        p.sort();
        p
    }

    fn build_now(index: &SharedIndex) {
        let (root, epoch) = write(index).begin_build().expect("root set");
        let walk = walk_index(&root, MAX_INDEXED_FILES);
        write(index).apply_build(epoch, walk);
    }

    #[test]
    fn indexed_file_splits_name_from_path() {
        let f = IndexedFile::new("a/b/main.rs".to_string());
        assert_eq!(f.name(), "main.rs");
        assert_eq!(f.path(), "a/b/main.rs");
        let root = IndexedFile::new("top.rs".to_string());
        assert_eq!(root.name(), "top.rs");
    }

    #[test]
    fn build_populates_index_and_search_ranks() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "src/main.rs", "x");
        write_file(dir.path(), "src/lib.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        let hits = write(&index).search("main", 10);
        assert_eq!(hits[0].path, "src/main.rs");
        assert_eq!(write(&index).status().file_count, 2);
    }

    #[test]
    fn patch_upserts_added_file() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        write_file(dir.path(), "b.rs", "x");
        on_workspace_changed(&index, dir.path().join("b.rs").to_str().unwrap(), false);
        assert_eq!(
            paths(&write(&index)),
            vec!["a.rs".to_string(), "b.rs".to_string()]
        );
    }

    #[test]
    fn patch_removes_deleted_file() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        write_file(dir.path(), "b.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        let removed = dir.path().join("a.rs");
        fs::remove_file(&removed).unwrap();
        on_workspace_changed(&index, removed.to_str().unwrap(), true);
        assert_eq!(paths(&write(&index)), vec!["b.rs".to_string()]);
    }

    #[test]
    fn patch_handles_two_path_rename() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "old.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        // A rename surfaces as two events: remove old, add new.
        let old = dir.path().join("old.rs");
        let new = dir.path().join("new.rs");
        fs::rename(&old, &new).unwrap();
        on_workspace_changed(&index, old.to_str().unwrap(), true);
        on_workspace_changed(&index, new.to_str().unwrap(), false);
        assert_eq!(paths(&write(&index)), vec!["new.rs".to_string()]);
    }

    #[test]
    fn patch_ignores_new_directories() {
        // The watcher fires WorkspaceChanged for a `mkdir` too; a directory must
        // never enter the file-name index.
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        let sub = dir.path().join("newdir");
        fs::create_dir(&sub).unwrap();
        on_workspace_changed(&index, sub.to_str().unwrap(), false);
        assert_eq!(paths(&write(&index)), vec!["a.rs".to_string()]);
    }

    #[test]
    fn patch_ignores_gitignored_additions() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), ".gitignore", "secret.txt\n");
        write_file(dir.path(), "keep.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        write_file(dir.path(), "secret.txt", "x");
        on_workspace_changed(
            &index,
            dir.path().join("secret.txt").to_str().unwrap(),
            false,
        );
        assert!(!paths(&write(&index)).contains(&"secret.txt".to_string()));
    }

    #[test]
    fn full_rebuild_reflects_disk_after_rescan() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);
        assert_eq!(write(&index).status().file_count, 1);

        // Bulk change on disk, then a full rebuild (the rescan path).
        write_file(dir.path(), "b.rs", "x");
        write_file(dir.path(), "c.rs", "x");
        fs::remove_file(dir.path().join("a.rs")).unwrap();
        build_now(&index);
        assert_eq!(
            paths(&write(&index)),
            vec!["b.rs".to_string(), "c.rs".to_string()]
        );
    }

    #[test]
    fn event_arriving_mid_build_survives_the_swap() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));

        // Simulate: a build is in flight (begin_build taken, walk not yet
        // applied).
        let (root, epoch) = write(&index).begin_build().unwrap();
        let walk = walk_index(&root, MAX_INDEXED_FILES);

        // A watcher event lands during the build. It cannot patch (building), so
        // it must arm a rebuild.
        write_file(dir.path(), "b.rs", "x");
        on_workspace_changed(&index, dir.path().join("b.rs").to_str().unwrap(), false);
        assert!(
            write(&index).rebuild_needed,
            "mid-build event must arm a rebuild"
        );

        // The swap completes and reports that another walk is required.
        let again = write(&index).apply_build(epoch, walk);
        assert!(again, "swap must re-arm because an event arrived mid-build");
        assert!(
            !write(&index).rebuild_needed,
            "rebuild_needed cleared after re-arm"
        );

        // The re-run picks up the file added mid-build.
        let walk2 = walk_index(&root, MAX_INDEXED_FILES);
        write(&index).apply_build(epoch, walk2);
        assert_eq!(
            paths(&write(&index)),
            vec!["a.rs".to_string(), "b.rs".to_string()]
        );
    }

    #[test]
    fn stale_build_swap_is_ignored_after_root_change() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        let (root, stale_epoch) = write(&index).begin_build().unwrap();
        let walk = walk_index(&root, MAX_INDEXED_FILES);

        // The workspace root changes before the stale walk is applied.
        let dir2 = TempDir::new().unwrap();
        write_file(dir2.path(), "z.rs", "x");
        write(&index).set_root(Some(dir2.path().to_path_buf()));

        // Applying the stale walk must not clobber the new root's index.
        let again = write(&index).apply_build(stale_epoch, walk);
        assert!(!again);
        assert!(
            paths(&write(&index)).is_empty(),
            "stale swap must be dropped"
        );
    }

    #[test]
    fn set_root_to_none_clears_index() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);
        write(&index).set_root(None);
        assert!(!write(&index).status().has_workspace);
        assert_eq!(write(&index).status().file_count, 0);
    }
}
