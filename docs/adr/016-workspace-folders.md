# ADR-016: Workspace Folders

**Status:** Accepted
**Date:** 2026-06-12

## Context

Writ's sidebar today shows Active tabs and History. Users working on a project need to browse the project tree without leaving the editor. The workspace primitive adds a single "root folder" concept: one directory opened at a time, browsable as a lazy-loading tree.

## Decisions

### Persistence — `WritConfig.workspace.root`

The workspace root is a scalar string (absolute path). It belongs in `config.toml` alongside other persistent preferences. SQLite would add schema churn for a single field. The existing `ConfigStore` + serde-default pattern handles missing values cleanly. A missing `[workspace]` section defaults to `root = null` with no migration cost.

### Policy in `writ-core`, I/O in `writ-storage`

- `writ-core/workspace` owns: `WorkspaceEntry` type, default ignore set, `is_ignored` predicate, `sort_entries` comparator. These are pure, no I/O, and fully testable without disk.
- `writ-storage/workspace_store` owns: `list_dir` — reads the directory, applies policy from core.
- `src-tauri/commands/workspace` delegates to storage; contains no policy.

This preserves the crate boundary rule: writ-core is pure, writ-storage owns all I/O.

### Traversal safety

At each `list_dir` call: canonicalize the workspace root and the requested directory path. Assert `canonical_dir.starts_with(canonical_root)`. `Path::canonicalize` resolves symlinks, so a symlink pointing outside the root resolves to a path outside and fails the prefix check. Tests cover: `..` escape attempt, symlink-to-external-dir escape, non-existent root, non-existent directory.

### Default ignore set

`.git`, `node_modules`, `target`, `dist`, `.DS_Store`, `.next`, `build`, `__pycache__`, `.cache`, `coverage`, `vendor`

These are applied in `writ-core::workspace::is_ignored` and enforced by `writ-storage::workspace_store::list_dir`.

### Watcher integration

A separate `start_workspace_watcher` function starts a `notify` recursive watcher on the workspace root and emits `writ://workspace-changed` events via Tauri's `AppHandle`. The returned `WorkspaceWatcherHandle` (wrapping a `Debouncer<RecommendedWatcher>`) is stored in `AppState.workspace_watcher: Mutex<Option<WorkspaceWatcherHandle>>`. Setting a root starts this watcher; clearing drops it.

Only `Create` and `Delete` file-system events propagate; `Modify` events are suppressed (content changes do not affect the file tree structure).

The existing `start_file_watcher` signature and `EventBus`-based dispatch for config + buffer changes is unchanged.

### Frontend

- `workspaceStore` singleton (`createRoot`): root signal, per-dir entry cache (Map keyed by path), `loadRoot`, `setRoot`, `clearRoot`, `listDir`, `invalidateDir`.
- `FileTree` component: `role="tree"`, `role="treeitem"`, `aria-expanded`, `aria-level`, roving tabindex, arrow-key navigation (Up/Down traverses visible items; Right expands; Left collapses). File click calls `bufferStore.openFile(path)` — the existing open-file path.
- Sidebar Files section shown above Active Tabs when root is set; "Open Folder…" affordance shown when not set.
- "Open Folder…" registered as `workspace.openFolder` command in the command palette.

## Consequences

- Config writes from `set_workspace_root` trigger `config:changed`. The frontend receives `workspace:root-changed` directly from the command; the config-changed event is a no-op for the workspace section.
- No multi-root support. ADR superseded when multi-root is needed.
- Watching a large tree is bounded only by OS file-descriptor limits; the default ignore set is the primary mitigation.

### Relationship to ADR-010

ADR-010's folder-as-workspace design was superseded when the preview surface
descoped to offline agent output: the `writ-workspace://` sibling-file
protocol, the file index, and the workspace switcher remain cut. This ADR
revives only the daily-driver subset — a browsable root folder in the sidebar
— with a different driver: working on prompts and notes that live next to
each other on disk. Nothing here feeds the preview trust model.

### Open authorization

File opens are origin-gated (`authorize_open`): a path must arrive through
the OS dialog or a window drop. Choosing a workspace folder through the OS
folder dialog expresses the same user intent for everything under it, so
`authorize_open` accepts any canonical path inside the workspace root. Paths
outside the root stay rejected. Covered by the origin-gate test suite.

### Change events

A dedicated recursive watcher (own debouncer, 500 ms) watches the root and
emits `WritEvent::WorkspaceChanged { path, removed }` onto the event bus;
the bridge forwards it as `writ://workspace-changed`. Events under ignored
directories are suppressed in core policy (`path_has_ignored_component`).
No self-write suppression is needed: buffer saves land in the buffers dir,
and a save-to-source merely triggers an idempotent re-listing of one
directory. The frontend reloads a directory listing only if it is already
cached; nothing is reloaded eagerly.
