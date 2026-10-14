//! In-memory workspace file-name index (ADR-026, C2).
//!
//! Holds one entry per workspace file: a relative-path `String` plus a `u32`
//! offset to its file-name segment, so a 200k-file index costs one string per
//! file rather than two: the lookup table holds positions keyed by a hash of
//! the path, never a second copy of it (TS-36). The index is built by a
//! bounded walk (in `writ-storage`) on workspace open and maintained
//! incrementally from the recursive workspace watcher: a changed path that
//! still exists and passes the union ignore policy is upserted, a gone path is
//! removed. The policy for one root is read once ([`WorkspaceIgnore`]) and
//! asked before the write lock is taken (TS-12).
//!
//! ## Build-vs-watcher race
//!
//! The initial walk (up to [`MAX_INDEXED_FILES`] files) runs on a background
//! thread and swaps its result into the shared index under the write lock. A
//! watcher event that lands while a build is in flight cannot be applied as a
//! patch (the entries are about to be replaced), so it is queued and replayed
//! in arrival order after the swap (TS-13). A queue past
//! [`MAX_PENDING_CHANGES`] is dropped for one more walk instead, so a tree
//! that keeps changing costs at most two walks per build rather than a walk
//! per write. A `build_epoch` supersedes any in-flight build when the root
//! changes, so a stale walk can never clobber a newer one.

use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use hashbrown::HashTable;
use writ_core::workspace::file_search::{rank_file_hits, FileHit};
use writ_storage::workspace_search::{walk_index, IndexWalk, WorkspaceIgnore};

/// Upper bound on indexed files; past it the index is marked truncated.
pub const MAX_INDEXED_FILES: usize = 200_000;

/// Upper bound on changes queued during one build; past it the build walks
/// once more instead of replaying them.
pub const MAX_PENDING_CHANGES: usize = 4_096;

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

/// One change the watcher reported, as the index applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// A file the index keeps exists at this relative path.
    Upsert(String),
    /// Nothing is at this relative path any more.
    Remove(String),
}

/// The workspace file-name index.
pub struct WorkspaceIndex {
    root: Option<PathBuf>,
    /// The root's ignore rules, read once per root rather than per event.
    ignore: Option<Arc<WorkspaceIgnore>>,
    entries: Vec<IndexedFile>,
    /// Position in `entries` of each path, keyed by the path's hash; the path
    /// itself is only in `entries`.
    positions: HashTable<usize>,
    hasher: RandomState,
    truncated: bool,
    /// Bumped on every successful build swap; the file-name search uses it, and
    /// tests observe rebuilds through it.
    generation: u64,
    /// A background build is in flight.
    building: bool,
    /// The running build must walk again after its swap: a sweep or a rule
    /// change arrived, or more changes than the queue holds.
    rebuild_needed: bool,
    /// Changes that arrived during the build, replayed after its swap.
    pending: Vec<Change>,
    /// Identifies the current build; a swap from a stale epoch is ignored.
    build_epoch: u64,
}

impl WorkspaceIndex {
    /// Creates an empty index for `root` (which may be `None`).
    pub fn new(root: Option<PathBuf>) -> Self {
        let ignore = root.as_deref().map(|r| Arc::new(WorkspaceIgnore::new(r)));
        Self {
            root,
            ignore,
            entries: Vec::new(),
            positions: HashTable::new(),
            hasher: RandomState::new(),
            truncated: false,
            generation: 0,
            building: false,
            rebuild_needed: false,
            pending: Vec::new(),
            build_epoch: 0,
        }
    }

    /// Replaces the workspace root, clearing all entries and superseding any
    /// in-flight build.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        let ignore = root.as_deref().map(|r| Arc::new(WorkspaceIgnore::new(r)));
        self.set_root_with(root, ignore);
    }

    fn set_root_with(&mut self, root: Option<PathBuf>, ignore: Option<Arc<WorkspaceIgnore>>) {
        self.root = root;
        self.ignore = ignore;
        self.entries.clear();
        self.positions.clear();
        self.truncated = false;
        self.building = false;
        self.rebuild_needed = false;
        self.pending.clear();
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

    /// Where `rel` sits in `entries`, if the index holds it.
    fn position(&self, rel: &str) -> Option<usize> {
        let hash = self.hasher.hash_one(rel);
        self.positions
            .find(hash, |&i| self.entries[i].path == rel)
            .copied()
    }

    /// Rebuilds `positions` from `entries`.
    fn reindex_positions(&mut self) {
        let Self {
            positions,
            entries,
            hasher,
            ..
        } = self;
        positions.clear();
        positions.reserve(entries.len(), |&i| {
            hasher.hash_one(entries[i].path.as_str())
        });
        for (i, entry) in entries.iter().enumerate() {
            positions.insert_unique(hasher.hash_one(entry.path.as_str()), i, |&j| {
                hasher.hash_one(entries[j].path.as_str())
            });
        }
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
        self.pending.clear();
        self.build_epoch += 1;
        Some((root, self.build_epoch))
    }

    /// Applies a completed walk if `epoch` is still current, then replays the
    /// changes that arrived during it. Returns `true` when another walk is
    /// required (the build stays armed); returns `false` when the build is done
    /// or was superseded.
    fn apply_build(&mut self, epoch: u64, walk: IndexWalk) -> bool {
        if epoch != self.build_epoch {
            // Superseded by a newer build or a root change; drop this result.
            return false;
        }
        self.entries = walk.paths.into_iter().map(IndexedFile::new).collect();
        self.reindex_positions();
        self.truncated = walk.truncated;
        self.generation += 1;

        if self.rebuild_needed {
            self.rebuild_needed = false;
            self.pending.clear();
            true
        } else {
            for change in std::mem::take(&mut self.pending) {
                self.apply_change(change);
            }
            self.building = false;
            false
        }
    }

    /// Holds `change` for the swap of the build in flight.
    fn queue(&mut self, change: Change) {
        if self.rebuild_needed {
            return;
        }
        if self.pending.len() >= MAX_PENDING_CHANGES {
            self.pending.clear();
            self.rebuild_needed = true;
            return;
        }
        self.pending.push(change);
    }

    fn apply_change(&mut self, change: Change) {
        match change {
            Change::Upsert(rel) => self.upsert(rel),
            Change::Remove(rel) => self.remove(&rel),
        }
    }

    /// Adds `rel` to the index if absent (respecting the cap).
    fn upsert(&mut self, rel: String) {
        if self.position(&rel).is_some() {
            return;
        }
        if self.entries.len() >= MAX_INDEXED_FILES {
            self.truncated = true;
            return;
        }
        let idx = self.entries.len();
        let hash = self.hasher.hash_one(rel.as_str());
        self.entries.push(IndexedFile::new(rel));
        let Self {
            positions,
            entries,
            hasher,
            ..
        } = self;
        positions.insert_unique(hash, idx, |&j| hasher.hash_one(entries[j].path.as_str()));
    }

    /// Removes `rel` from the index if present.
    fn remove(&mut self, rel: &str) {
        let hash = self.hasher.hash_one(rel);
        let Self {
            positions,
            entries,
            hasher,
            ..
        } = self;
        let Ok(found) = positions.find_entry(hash, |&i| entries[i].path == rel) else {
            return;
        };
        let (idx, _) = found.remove();
        let last = entries.len() - 1;
        entries.swap_remove(idx);
        if idx != last {
            // The element that was at `last` now sits at `idx`; repoint it.
            let moved = hasher.hash_one(entries[idx].path.as_str());
            if let Some(slot) = positions.find_mut(moved, |&i| i == last) {
                *slot = idx;
            }
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

/// Locks the shared index for reading, recovering from poisoning.
fn read(index: &SharedIndex) -> std::sync::RwLockReadGuard<'_, WorkspaceIndex> {
    index.read().unwrap_or_else(|e| {
        tracing::error!(
            location = "workspace_index::read",
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
/// when a sweep, a rule change or an overflowing queue arrived mid-build.
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
    // Read before the lock: it opens the root's rule files.
    let ignore = root.as_deref().map(|r| Arc::new(WorkspaceIgnore::new(r)));
    write(index).set_root_with(root, ignore);
    if has_root {
        spawn_rebuild(index.clone());
    }
}

/// Handles a `WorkspaceSwept` event: more changed than the watcher named,
/// possibly the root's own ignore rules among it, so the rules are read again
/// and the tree walked.
pub fn on_workspace_swept(index: &SharedIndex) {
    let Some(root) = read(index).root.clone() else {
        return;
    };
    reread_rules_and_rebuild(index, &root);
}

/// Reads `root`'s ignore rules again, outside the lock, and walks the tree.
fn reread_rules_and_rebuild(index: &SharedIndex, root: &Path) {
    let reread = Arc::new(WorkspaceIgnore::new(root));
    {
        let mut idx = write(index);
        if idx.root.as_deref() != Some(root) {
            return;
        }
        idx.ignore = Some(reread);
    }
    spawn_rebuild(index.clone());
}

/// Handles one `WorkspaceChanged` event against the index.
///
/// What the path is, and whether the index keeps it, is decided before the
/// write lock is taken, against the root's policy read once. During a build
/// the change is queued for the swap; otherwise the one path is patched in
/// place. A change to the root's own `.gitignore` or `.ignore` reads the policy
/// again and walks the tree, since it can change what every path answers.
pub fn on_workspace_changed(index: &SharedIndex, path: &str, removed: bool) {
    let abs = Path::new(path);
    let (root, ignore) = {
        let idx = read(index);
        match (idx.root.clone(), idx.ignore.clone()) {
            (Some(root), Some(ignore)) => (root, ignore),
            _ => return,
        }
    };
    let Some(rel) = relative(&root, abs) else {
        return;
    };

    if ignore.is_rules_file(abs) {
        reread_rules_and_rebuild(index, &root);
        return;
    }

    let change = if removed || !abs.exists() {
        let idx = read(index);
        if !idx.building && idx.position(&rel).is_none() {
            return;
        }
        Change::Remove(rel)
    } else if abs.is_file() && ignore.is_indexed(abs) {
        // The watcher fires for directory creation too and does not distinguish
        // it from a file; the index holds files only, matching the build walk.
        Change::Upsert(rel)
    } else {
        return;
    };

    let mut idx = write(index);
    if idx.root.as_deref() != Some(root.as_path()) {
        return;
    }
    if idx.building {
        idx.queue(change);
    } else {
        idx.apply_change(change);
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

    /// Starts a build and walks, leaving the swap to the test, which is the
    /// window a watcher event can land in.
    fn walk_in_flight(index: &SharedIndex) -> (u64, IndexWalk) {
        let (root, epoch) = write(index).begin_build().expect("root set");
        (epoch, walk_index(&root, MAX_INDEXED_FILES))
    }

    fn changed(index: &SharedIndex, dir: &Path, rel: &str, removed: bool) {
        on_workspace_changed(index, dir.join(rel).to_str().unwrap(), removed);
    }

    #[test]
    fn an_event_arriving_mid_build_is_replayed_after_the_swap_without_a_second_walk() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        let (epoch, walk) = walk_in_flight(&index);

        write_file(dir.path(), "b.rs", "x");
        changed(&index, dir.path(), "b.rs", false);

        let again = write(&index).apply_build(epoch, walk);
        assert!(
            !again,
            "a queued change is replayed, not walked for (TS-13)"
        );
        assert_eq!(
            paths(&write(&index)),
            vec!["a.rs".to_string(), "b.rs".to_string()]
        );
    }

    #[test]
    fn a_removal_during_a_build_is_replayed_after_the_swap() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        write_file(dir.path(), "b.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        let (epoch, walk) = walk_in_flight(&index);

        fs::remove_file(dir.path().join("a.rs")).unwrap();
        changed(&index, dir.path(), "a.rs", true);

        assert!(!write(&index).apply_build(epoch, walk));
        assert_eq!(paths(&write(&index)), vec!["b.rs".to_string()]);
    }

    #[test]
    fn an_ignored_path_during_a_build_causes_no_rewalk() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), ".gitignore", "secret.txt\n");
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        let (epoch, walk) = walk_in_flight(&index);

        write_file(dir.path(), "node_modules/pkg/index.js", "x");
        write_file(dir.path(), ".venv/lib/site.py", "x");
        write_file(dir.path(), "secret.txt", "x");
        changed(&index, dir.path(), "node_modules/pkg/index.js", false);
        changed(&index, dir.path(), ".venv/lib/site.py", false);
        changed(&index, dir.path(), "secret.txt", false);
        assert!(write(&index).pending.is_empty());

        assert!(!write(&index).apply_build(epoch, walk));
        assert_eq!(
            paths(&write(&index)),
            vec![".gitignore".to_string(), "a.rs".to_string()]
        );
    }

    #[test]
    fn continuous_events_during_a_build_converge_within_two_walks() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        let (epoch, walk) = walk_in_flight(&index);

        // One more change than the queue holds: the build walks once more,
        // and the second swap has nothing left to replay.
        write_file(dir.path(), "dev.log", "x");
        for _ in 0..=MAX_PENDING_CHANGES {
            changed(&index, dir.path(), "dev.log", false);
        }
        assert!(
            write(&index).apply_build(epoch, walk),
            "overflow walks once more"
        );
        let walk2 = walk_index(dir.path(), MAX_INDEXED_FILES);
        assert!(!write(&index).apply_build(epoch, walk2), "and then stops");
        assert_eq!(
            paths(&write(&index)),
            vec!["a.rs".to_string(), "dev.log".to_string()]
        );
    }

    #[test]
    fn a_change_to_the_roots_ignore_rules_is_read_again() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "keep.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        write_file(dir.path(), ".gitignore", "secret.txt\n");
        changed(&index, dir.path(), ".gitignore", false);
        write_file(dir.path(), "secret.txt", "x");
        let ignore = write(&index).ignore.clone().expect("policy");
        assert!(!ignore.is_indexed(&dir.path().join("secret.txt")));
    }

    #[test]
    fn a_sweep_reads_the_roots_ignore_rules_again() {
        // The rule file's own event may be one the budget folded into the
        // sweep, so the sweep is what has to read it.
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "keep.rs", "x");
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);

        write_file(dir.path(), ".gitignore", "secret.txt\n");
        write_file(dir.path(), "secret.txt", "x");
        on_workspace_swept(&index);
        let ignore = write(&index).ignore.clone().expect("policy");
        assert!(!ignore.is_indexed(&dir.path().join("secret.txt")));
    }

    #[test]
    fn positions_follow_every_entry_through_removals() {
        let dir = TempDir::new().unwrap();
        for i in 0..50 {
            write_file(dir.path(), &format!("f{i}.rs"), "x");
        }
        let index: SharedIndex = Arc::new(RwLock::new(WorkspaceIndex::new(Some(
            dir.path().to_path_buf(),
        ))));
        build_now(&index);
        for i in (0..50).step_by(3) {
            let rel = format!("f{i}.rs");
            fs::remove_file(dir.path().join(&rel)).unwrap();
            changed(&index, dir.path(), &rel, true);
        }

        let idx = write(&index);
        assert_eq!(idx.entries.len(), 33);
        for (i, entry) in idx.entries.iter().enumerate() {
            assert_eq!(idx.position(entry.path()), Some(i), "{}", entry.path());
        }
        assert_eq!(idx.position("f0.rs"), None);
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
