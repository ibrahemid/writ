# ADR-018: Watch Inbox

**Status:** Accepted
**Date:** 2026-06-13

## Context

Developers point their tools at a folder — a `reports/` directory a test runner writes into, a `.status/` directory a long-running task drops summaries into. Today the loop is: task finishes, switch to a file manager or terminal, find the new file, open it. The Watch Inbox closes that loop: the user designates one folder; when a new openable file appears there, Writ opens it automatically, and renderable types (`.md`, `.html`) land directly in their rendered preview layout.

## Decisions

### Persistence — `WritConfig.inbox`

`[inbox]` is a config section with two fields: `path` (absolute path, `null` when no inbox is watched) and `focus` (`true` by default). Same reasoning as ADR-016's `workspace.root`: a scalar preference belongs in `config.toml`, serde defaults make a missing section cost-free, and the existing `ConfigStore` round-trips it.

### Only files created after watching began auto-open

Enabling the inbox on a folder with 400 existing reports must not open 400 tabs. The qualifying check compares the file's creation timestamp (birth time; modification time on filesystems without birth time) against the instant the watcher started. Pre-existing files — including pre-existing files that are merely *modified* while watched — never auto-open. The debouncer does not distinguish create from modify, so the timestamp comparison is the create/modify discriminator as well as the backlog guard.

### Policy in `writ-core`, mechanism in the adapter

- `writ-core::inbox::qualifies_for_auto_open(root, path, created, watch_start)` is pure: containment under the inbox root, the shared default ignore set (`workspace::path_has_ignored_component` — `node_modules` churn inside an inbox never opens tabs), and the created-after-watch-start rule.
- `src-tauri::watcher::handler::classify_inbox_event` is the mechanism: reads file metadata, applies core policy, then gates on the existing `file_ops::validate_file_for_opening` (regular file, ≤ 10 MiB, non-binary). Directories, deletions, binaries, and oversized files are suppressed.

### Which files qualify, and how they open

Anything passing `validate_file_for_opening` opens as a buffer through the existing open path. No new layout code: `PreviewLayout` already resolves a freshly opened buffer's layout as persisted → content-type config default → source, so `.md`/`.html` arrivals render with `preview.default_layout_markdown` / `default_layout_html` exactly as a dialog-opened file would. The persistent preview pane is never recreated; arrivals navigate it like any other open.

### Burst cap

At most 3 auto-opens per 2-second window. Further arrivals inside the window collapse into a single toast — "N new files in inbox" — instead of N tabs. The cap lives in the frontend inbox store (the entity that opens tabs is the entity that rations them); the watcher emits one `InboxFileArrived` per qualifying file and stays policy-free about tab pressure.

### Focus policy — `inbox.focus`, default `true`

On auto-open Writ unminimizes, shows, and focuses its window. This is the point of the feature: the terminal task finishes and the rendered report is *in front of you*. The tradeoff is real — focus stealing while typing elsewhere is hostile — so `inbox.focus = false` keeps arrivals silent in the background, and the toggle ships in Settings → Files. Default stays `true` because an inbox is an explicit, recent, per-folder opt-in, not an ambient daemon.

### Authorization

File opens remain origin-gated (`authorize_open`). Choosing the inbox folder through the OS folder dialog expresses user intent for files under it, exactly as the workspace root does (ADR-016): `AppState.is_within_inbox` accepts any canonical path inside the inbox root, alongside the existing dialog/drop and workspace checks. Paths outside both roots stay rejected. The gate is not weakened in any other way; containment is covered by the origin-gate test suite.

### Self-write safety and dedupe

Writ never writes into inbox directories itself (buffer saves land in the buffers dir), so no ignore-stamp suppression is needed. A user may point the inbox at a folder containing already-open files; the existing open path dedupes by canonical `source_path` (`find_active_by_source_path`), so an arrival for an open file activates the existing tab instead of duplicating it.

### Watcher lifecycle

`start_inbox_watcher` mirrors `start_workspace_watcher`: its own 500 ms debouncer, recursive watch, `WatcherHandle` stored in `AppState.inbox_watcher`. Picking a folder starts it; clearing drops it; app launch restores it from config when the persisted path still exists. The bridge forwards `WritEvent::InboxFileArrived { path }` as `writ://inbox-file-arrived`.

## Consequences

- One inbox at a time, like the workspace root. Superseded when multiple inboxes are needed.
- Filesystems without birth time degrade to mtime: a pre-existing file modified while watched can auto-open there. Acceptable — macOS, Windows, and ext4 all report birth time.
- Pointing the inbox at a very active directory (a build output dir) is bounded by the ignore set, the text/size validation, and the burst cap; worst case is a periodic "N new files" toast.
- The inbox is orthogonal to the workspace: neither requires the other, and both extend the origin gate by folder containment.
