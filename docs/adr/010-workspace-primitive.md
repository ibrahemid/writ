# ADR-010: Workspace Primitive — Folder-as-Workspace and the `writ-workspace://` Protocol

**Status:** Superseded — descoped to offline agent-output preview (2026-05-26)
**Date:** 2026-05-23

> **Superseded.** The preview surface was descoped to a single purpose:
> rendering Writ's own agent/LLM output and prompt results, offline, for
> readability. It is explicitly **not** a safe renderer for hostile web HTML,
> and network egress is categorically off forever. That dissolves the reason
> this ADR existed: there are no sibling-file assets to resolve. Agent HTML is
> self-contained or uses `data:` URIs; inline images/SVG render via the HTML
> renderer under the fixed CSP. The `writ-workspace://` protocol, the
> folder-as-workspace model, the file index, and the workspace switcher are
> **cut** — not deferred. This document is retained as a record of the
> considered design. See the lean re-scope note in ADR-009.

## Context

ADR-009 commits the preview surface: a child Tauri webview, a content-type
renderer registry, a layout system spanning split/swap/detached, a per-window
state model, and a default-deny CSP. It defers two things. ADR-011 will own
the trust model, the per-scope CSP bytes, the pin storage, and the
verification corpus. **This ADR — ADR-010 — owns the second deferral: the
workspace primitive and the `writ-workspace://` sibling-file protocol.**

Today Writ has no workspace concept. A buffer is opened by absolute path; the
sidebar (per CLAUDE.md) shows an "IDE Active" group of currently-open buffers
and a time-bucketed "History" group of recently-closed buffers; the preview
surface (per ADR-009) renders the document under default-deny with zero
subresource resolution. That model is correct for one-off files opened from
Finder; it is wrong for the canonical case the preview surface ships for:
authoring an HTML or markdown document that references sibling files — a
stylesheet, a logo, a typeface, a hero image.

The right answer is not to relax the default-deny CSP or to make the preview
webview clever about sibling paths. The right answer is what every serious
editor has reached: a **workspace** is a directory on disk; opening it gives
the editor a stable root inside which relative references can be resolved;
resolution goes through a host-controlled protocol with an explicit
allowlist, never through `file://` and never through anything that would let
a document escape the workspace root.

This ADR commits the folder-as-workspace data model, the `writ-workspace://`
custom Tauri protocol handler with its strict allowlist and path-traversal
guard, the multi-workspace state model (app-global registry, per-window
active workspace, per ADR-009's E3 hybrid), the workspace-switcher binding
and its conflict resolution with ADR-009's keymap, the `.writ/`
per-workspace config directory, the file index strategy, the
promote-folder-to-workspace UX, the workspace-close semantics, and the
performance budgets any implementation must meet or fail in CI.

The trust-boundary aspect of workspaces is **stated** here, not specified.
ADR-011 owns the pin records, the trust state machine, the per-workspace
"trust all" scope, and the per-workspace trust dashboard. This ADR commits
only that workspaces are addressable as trust boundaries: a workspace has a
stable identity (`WorkspaceId`) that ADR-011's records can reference.

## Decision drivers

- **Safe-by-construction subresource resolution.** A document referencing
  `./styles.css` must resolve only inside the canonicalized workspace root.
  Path traversal (`..`), absolute paths, hostile symlinks, and any path that
  escapes the root after canonicalization are blocked at the protocol
  handler. No per-renderer logic, no per-component branch.
- **Allowlist over denylist.** The protocol handler serves a small,
  enumerated set of MIME types — stylesheets, images, fonts, JS bytes
  (execution remains gated by ADR-011's CSP). Anything outside the allowlist
  is a 404. Adding a type is a deliberate handler change.
- **Folder-as-workspace, not project-file-as-workspace.** A directory is a
  workspace because the user opened it as one. `.writ/` is the only
  first-party artifact and it is optional.
- **App-global registry, per-window active workspace.** Per ADR-009's E3
  hybrid: the registry of open workspaces is app-global state; the *active*
  workspace per window is per-window state. A user can have one workspace
  focused in window A and a different workspace focused in window B.
- **Third sidebar surface above the existing two.** The existing IDE Active
  and History panels are buffer-scoped and stay that way. The workspace
  panel is a third surface mounted above them, scoped by the active
  workspace.
- **`Cmd+R` is taken.** ADR-009 binds `Cmd+R` to force-render preview. The
  workspace switcher cannot use it; this ADR picks a non-colliding binding
  and codifies the resolution.
- **`Cmd+Shift+O` is dual-use.** ADR-009 already committed it to "detach
  preview when an active tab has a registered renderer, else open workspace
  picker." The precise precedence rule is codified here.
- **Crate boundaries hold.** Per ADR-005: `writ-core` gets pure-domain types
  with zero Tauri dependency; `writ-storage` owns persistence; `writ-plugin`
  is untouched (workspace is host-side, not a plugin surface); `src-tauri`
  owns the protocol handler and the AppState integration.
- **Cross-platform parity is a launch requirement.** macOS, Linux, and
  Windows handle canonicalization and symlinks differently; the protocol
  handler must behave identically on all three.

## Considered options

Seven composite decisions: **A** workspace identity, **B** multi-workspace
state shape, **C** protocol allowlist, **D** file index strategy, **E**
switcher binding, **F** `Cmd+Shift+O` precedence, **G** `.writ/` contents.

### A — Workspace identity model

#### A1. Directory-as-workspace (no required marker)

A workspace is any directory the user opens. `WorkspaceId` is derived
deterministically from the canonicalized absolute path of the root. Opening
the same directory twice in different sessions resolves to the same
`WorkspaceId`. The optional `.writ/` holds per-workspace config but does
not gate the directory's status as a workspace.

- Pros: zero ceremony; matches `code .` ergonomics; identity is stable
  because the canonical path is stable.
- Cons: the system has no opinion about which directories are "Writ
  projects." Mitigated by the recents list — the user's actual usage is
  the opinion.

#### A2. Marker-file-as-workspace (`.writproject` required)

A directory becomes a workspace only with a marker file at root. Opening
without one prompts to create one.

- Cons: hostile to the common case (drag a folder onto Writ). Marker
  files become noise in version control. Workspaces are a per-user,
  per-session artifact and do not have a publishable definition.
  **Rejected.**

#### A3. Hybrid (marker file optional, elevates trust)

Directory is a workspace by being opened; a marker file signals
additional intent and auto-loads config.

- Cons: two code paths for what should be one concept. The "additional
  intent" cases all reduce to features the recents list already
  provides. **Rejected.**

**Chosen: A1.** A directory becomes a workspace by being opened. No
marker file. `.writ/config.toml` is the optional shareable surface;
`.writ/` presence does not change workspace status. Identity is the
canonicalized absolute path; `WorkspaceId` is a stable hash of that
path.

### B — Multi-workspace state shape

#### B1. One workspace active per app

Single active workspace; opening a new one tears down the prior.

- Cons: hostile to multi-monitor and multi-project workflows. Fails the
  obvious case the moment a second window exists (which ADR-009's
  detached preview already makes routine). **Rejected.**

#### B2. Multiple workspaces open, one active per window

Registry holds every open workspace; active workspace is per-window. Two
windows can focus different workspaces simultaneously.

- Pros: maps exactly onto ADR-009's E3 hybrid (app-global state in
  `src/stores/global/`, per-window state in `src/stores/window/`). The
  lint rule from ADR-009 keeps the line honest. Multi-monitor workflows
  natively supported.
- Cons: the file index lives for every workspace in the registry. The
  perf section ceilings this: idle workspaces are evicted after 5
  minutes; the central DB rows survive so re-focus is a re-walk, not a
  re-index.

#### B3. Multiple workspaces per window (multi-root)

A single window with more than one workspace root active.

- Cons: introduces resolution ambiguity (which root does `./styles.css`
  mean?), grows the protocol handler's lookup, complicates ADR-011's
  per-workspace trust scope. The complication is worth having one day;
  it is not worth taking on as the v1 default. **Deferred to a future
  ADR** (multi-root workspaces).

**Chosen: B2.**

- `src/stores/global/workspaces.ts` — singleton registry of open
  workspaces (`WorkspaceId`, canonicalized path, display name,
  last-opened timestamp).
- `src/stores/window/active-workspace-store.ts` — per-window active
  workspace, instantiated inside each `<WindowProvider>` boundary (per
  ADR-009).

### C — Protocol handler allowlist

#### C1. Strict MIME allowlist with extension dispatch

The handler dispatches by lowercased extension after canonicalization.
The allowed set is enumerated:

| Category    | Extensions                                                | MIME / behavior                            |
|-------------|-----------------------------------------------------------|--------------------------------------------|
| Stylesheets | `.css`                                                    | `text/css; charset=utf-8`                  |
| Scripts     | `.js`, `.mjs`                                             | `application/javascript`; **execution gated by ADR-011's CSP** — workspace-readable does not mean document-runnable |
| Images      | `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.svg`, `.avif`, `.ico` | Type-correct MIME; EXIF stripped per ADR-009's raster-image hygiene |
| Fonts       | `.woff`, `.woff2`, `.ttf`, `.otf`                         | Type-correct MIME; `Cross-Origin-Resource-Policy: same-origin` |
| Everything else | —                                                     | `404 Not Found`; rejection logged at debug |

Path-traversal rules (applied before extension dispatch):

| Check                                                            | Failure         |
|------------------------------------------------------------------|-----------------|
| Path contains `..` segments after percent-decoding               | `403 Forbidden` |
| Path is absolute (`/...`, `C:\...`)                              | `403 Forbidden` |
| Canonicalized path does not start with canonicalized workspace root | `403 Forbidden` |
| Symlink resolves outside workspace root (canonicalize-then-prefix-check catches this) | `403 Forbidden` |
| Workspace root no longer exists on disk                          | `503` + UI banner |
| No active workspace for the requesting webview's window          | `404`           |

- Pros: serves exactly what documents legitimately reference; adding a
  type is a deliberate PR; the traversal guard is unit-testable.
- Cons: inconvenient for assets outside the allowlist — the right
  response is to convert the asset, not expand the allowlist
  carelessly.

#### C2. Configurable allowlist via `.writ/config.toml`

User extends the allowlist per workspace.

- Cons: gives a document the right to fetch arbitrary MIME from disk.
  The handler becomes a policy surface, which ADR-011 says it should
  not be. Trust belongs in pins, not in TOML files. **Rejected.**

#### C3. Open allowlist with extension blocklist

Anything except an explicit denylist (`.exe`, `.sh`, `.dylib`, ...) is
served.

- Cons: denylist-based security is the textbook anti-pattern. Every new
  extension is a potential bypass. **Rejected.**

**Chosen: C1.**

### D — File index strategy

#### D1. Reuse `writ-storage`'s FTS5, add `workspace_id` column

The existing FTS5 index (today indexes open buffer content) gains a
`workspace_id` column on the files table. On workspace open a background
worker walks the root, inserts a row per file, and feeds content into
FTS for files under a cap. Switching workspaces is a query filter, not a
re-index.

- Pros: one index; one query path for "search across the active
  workspace"; the same index feeds Cmd+P open-file and a future
  project-wide search.
- Cons: couples workspace open performance to FTS insertion cost.
  Mitigated by the size cap.

#### D2. Per-workspace SQLite under `.writ/.index.db`

Each workspace gets its own SQLite file.

- Cons: a binary artifact in the user's workspace that must be
  gitignored; two SQLite files compete for cache; multiple connections
  to manage. **Rejected.**

#### D3. In-memory index rebuilt on open, persisted snapshot

In-process index with periodic JSON snapshot at `.writ/.index-snapshot.json`.

- Cons: re-implements FTS5's lookup features; new bespoke format must
  be versioned and migrated. **Rejected.**

**Chosen: D1.** Specifics:

- **Per-file content cap:** files ≤ 1 MB have contents indexed into
  FTS5; larger files are name-indexed only.
- **Workspace size cap:** workspaces with ≤ 50,000 files index
  unconditionally. Above 50,000 a confirm appears:
  `"this workspace has N files; index anyway?"` with `Index`,
  `Name only`, `Cancel` actions. Choice persists in `.writ/config.toml`.
- **Off-thread:** walker runs on a Tokio worker; sidebar shows
  `indexing 1,247 / 8,000` chip until complete.
- **Incremental updates:** the existing file watcher emits events
  scoped to the workspace root; the indexer applies batched upserts
  within a 50 ms coalescing window, budgeted at < 50 ms per save
  (perf section).

### E — Workspace switcher binding

ADR-009 took `Cmd+R` for force-render. The original epic prompt's
`Cmd+R for workspace switching (VS Code style)` is therefore
unavailable. Three replacements:

#### E1. `Cmd+Shift+W`

Currently unbound. `Cmd+W` is close-tab; `Cmd+Shift+W` is not in the
existing keymap, the CodeMirror keymap, or the ADR-009 preview keymap.

- Pros: mnemonic (W for Workspace); adjacent to `Cmd+W` for muscle
  memory (the W-cluster is workspace-and-tab).
- Cons: some macOS apps use it for close-all-tabs; Writ does not.

#### E2. `Cmd+K Cmd+O` leader-key sequence

- Cons: Writ has no leader-key prefix today; introducing one for a
  single binding is a pattern shift, not a feature decision. If a
  leader prefix is the right answer for workspace actions, it is the
  right answer for a larger set, which is a separate ADR. **Rejected.**

#### E3. Repurpose `Cmd+P` with shift-prefix

Wire `Cmd+P` to scope to active workspace; use a separate binding for
switching.

- Cons: conflates open-file with switch-workspace; they are different
  operations. **Rejected.**

**Chosen: E1 — `Cmd+Shift+W`.** Opens the workspace switcher dropdown
anchored to the sidebar's workspace header. Lists the active workspace,
other open workspaces, recent workspaces from disk, an `Open folder…`
action, and a `Close workspace` action. The sidebar workspace header
itself is the click-equivalent target. Conflict resolution:

| Original proposal      | Conflict                              | Resolution                          |
|------------------------|---------------------------------------|-------------------------------------|
| `Cmd+R` switches workspace | ADR-009 binds `Cmd+R` to force-render | `Cmd+R` stays on preview; switcher moves to `Cmd+Shift+W` |
| Match VS Code's `Ctrl+R`   | VS Code recent-folders binding        | Writ's `Cmd+Shift+W` is the documented binding; the shortcut editor is the user's escape hatch if they want to remap |

### F — `Cmd+Shift+O` precedence

ADR-009 committed `Cmd+Shift+O` to "detach preview when an active tab
has a registered renderer, else open workspace picker." The ambiguous
case is a tab whose content type has a registered renderer but where
the user might mean either action. Codified precedence:

| Active tab state                                                                | `Cmd+Shift+O` action |
|---------------------------------------------------------------------------------|----------------------|
| Has registered renderer **and** preview pane mounted (`Split`/`Preview`/`Detached`) | Detach (ADR-009)     |
| Has registered renderer **and** layout is `Source`                              | Detach (ADR-009) — detach mounts preview in the new window |
| Has registered renderer **and** is already `Detached` from this window          | Re-attach (ADR-009 symmetric binding from detached window) |
| No active tab (empty window or welcome state)                                   | Open workspace picker (this ADR) |
| Active tab has no registered renderer (e.g., `.rs` source file)                 | Open workspace picker (this ADR) |
| Active tab is the welcome/empty buffer                                          | Open workspace picker (this ADR) |

**Rule:** an active tab with a registered renderer **always wins** for
detach. Opening a workspace requires no-active-tab, the welcome state,
or a non-renderer tab. The shortcut editor documents the precedence
inline with the table above as help text.

Unambiguous workspace-open paths for the common case:

- Sidebar workspace header dropdown → `Open folder…`.
- Command palette (`Shift+Shift`) → `Open workspace`.
- OS file menu (menubar item).
- `Cmd+Shift+W` switcher → `Open folder…`.

`Cmd+Shift+O` is the fast-path; the four paths above are the
unambiguous fallbacks. This is the only context-sensitive binding in
the keymap.

### G — `.writ/` directory contents

#### G1. Minimal v1: config + reserved keys

`.writ/config.toml` holds per-workspace overrides for a defined subset
of `WritConfig` keys. `.writ/pins.toml` is reserved as a file path but
not populated by this ADR (ADR-011 owns it). `.writ/.gitignore` ships
as an optional template via a one-click affordance.

Override **allowlist** (only keys that may appear in
`.writ/config.toml`):

| Key                                  | Default                | Scope         |
|--------------------------------------|------------------------|---------------|
| `preview.default_layout_html`        | `split`                | Per workspace |
| `preview.default_layout_markdown`    | `split`                | Per workspace |
| `preview.default_layout_pdf`         | `preview`              | Per workspace |
| `preview.default_layout_image`       | `preview`              | Per workspace |
| `preview.default_layout_svg`         | `preview`              | Per workspace |
| `preview.debounce_ms`                | `200`                  | Per workspace |
| `preview.live_render_threshold_mb`   | `1`                    | Per workspace |
| `preview.render_confirm_threshold_mb`| `5`                    | Per workspace |
| `preview.render_refuse_threshold_mb` | `50`                   | Per workspace |
| `index.size_limit`                   | `50000`                | Per workspace |
| `index.content_size_cap_mb`          | `1`                    | Per workspace |

Override **denylist** (these may not appear; reading produces a logged
warning and the workspace falls back to global config):

| Forbidden key                | Reason                                                |
|------------------------------|-------------------------------------------------------|
| `preview.policy.*` (CSP-relevant) | Trust lives in ADR-011's pins, not per-workspace TOML. A repo cannot vendor its own CSP relaxations |
| `trust.*`                    | Same reason; ADR-011 owns trust state                 |
| `update.*`                   | Update channel is per-user                            |
| `keymap.*`                   | Keybindings are per-user                              |
| `auth.*`                     | If/when an auth surface exists, it is per-user        |

Explicitly **not** stored in `.writ/` in v1:

- Session state (buffer list, cursor positions, undo history) — lives
  in the central SQLite store keyed by `workspace_id`.
- Recent files — captured by the FTS file index and the existing
  history surface.
- The file-index snapshot — D1 won; no `.index.db`.

#### G2. Maximal v1: everything in `.writ/`

- Cons: user's source-controlled workspace now contains binary or
  semi-binary user-specific state that should not be checked in; two
  stores (central DB and `.writ/`) overlap and drift. **Rejected.**

#### G3. Nothing in `.writ/` — pure central DB

- Cons: forecloses on shareable per-workspace settings; a user cannot
  commit `.writ/config.toml` to standardize preview defaults for
  collaborators. **Rejected.**

**Chosen: G1.** Optional `.writ/.gitignore` template, if the user
clicks the affordance:

```
# Writ per-machine files (do not commit)
.writ/.session/
.writ/.cache/
```

Neither directory is created in v1; the template gitignores them
prophylactically for future per-workspace caches.

## Decision (composite)

- **Identity:** **A1** — directory-as-workspace; `WorkspaceId` derived
  from a stable hash of the canonicalized absolute path.
- **State shape:** **B2** — app-global registry in
  `src/stores/global/workspaces.ts`, per-window active workspace in
  `src/stores/window/active-workspace-store.ts`, aligned with ADR-009's
  E3 hybrid.
- **Allowlist:** **C1** — strict MIME allowlist (CSS, JS bytes pending
  ADR-011 CSP, images, fonts). Traversal guard:
  canonicalize-then-prefix-check; reject `..`, absolute paths, hostile
  symlinks; 404 for unknown types; 403 for traversal; 503 for
  unreachable root.
- **File index:** **D1** — reuse FTS5; add `workspace_id` column;
  50k-file cap with confirm; 1 MB per-file content cap; off-thread
  walk; progress chip.
- **Switcher binding:** **E1** — `Cmd+Shift+W`. `Cmd+R` stays on
  preview force-render per ADR-009.
- **`Cmd+Shift+O` precedence:** **F** — active-renderer tab always
  wins for detach; workspace-open requires no-renderer context.
- **`.writ/` contents:** **G1** — minimal: `config.toml` with
  allowlist + denylist; `pins.toml` reserved for ADR-011; `.gitignore`
  optional template.

## Keymap

| Binding             | Action                                         | Scope          |
|---------------------|------------------------------------------------|----------------|
| `Cmd+Shift+W`       | Open workspace switcher dropdown               | Active window  |
| `Cmd+Shift+O`       | Detach preview (if renderer active) **or** open workspace picker (see F) | Active window |
| `Cmd+O`             | Open file dialog (unchanged; scoped to active workspace root when one exists) | Active window |
| `Cmd+Shift+P` → `Open workspace…` | Open OS folder picker from command palette | Active window |
| `Cmd+W`             | Close active tab (unchanged)                   | Active tab     |
| Click sidebar workspace name | Open workspace switcher dropdown          | Active window  |

`Cmd+Shift+W` is conflict-checked against CodeMirror's default keymap,
the ADR-009 preview keymap, and the existing app keymap
(`Cmd+Shift+Space`, `Cmd+T`, `Cmd+W`, `Cmd+[`/`Cmd+]`, `Cmd+SS`,
`Shift+Shift`) — all unused.

## Sidebar topology

A third surface above the existing two, top to bottom:

1. **Workspace panel** (new — this ADR). Renders only when a workspace
   is active on the window.
   - Header: workspace display name + downward chevron. Clicking the
     header or pressing `Cmd+Shift+W` opens the switcher.
   - Body: lazy-loaded file tree rooted at workspace root, filtered by
     `.gitignore`.
   - Footer chip: indexing progress or "ready" state.
2. **IDE Active panel** (existing). Unchanged.
3. **History panel** (existing). Unchanged.

When no workspace is active, the Workspace panel collapses to:

```
no workspace open · open folder…
```

The IDE Active and History panels function in the no-workspace branch
exactly as today. Workspace context, when present, augments the
sidebar; it does not replace the buffer-scoped surfaces.

## Workspace switcher

A dropdown anchored to the workspace header. Sections:

| Section            | Contents                                                                |
|--------------------|-------------------------------------------------------------------------|
| Active             | The currently focused workspace (this window).                          |
| Other open         | Workspaces in the registry not active in this window. Clicking switches this window's active workspace. |
| Recents            | Up to N (default 10) workspaces from the `workspaces` table not in the open registry. Clicking opens and focuses. |
| Open folder…       | Triggers the OS folder picker.                                          |
| Close workspace    | Removes the active workspace from this window's focus; sidebar collapses to the empty row. |

Recents pruning: max N most-recently-opened entries. Deletion is a
context-menu item (`Forget this workspace`), which removes the row from
the table but does not touch disk.

## Workspace persistence

Workspaces persist across launches via a `workspaces` table:

```sql
CREATE TABLE workspaces (
    id                     INTEGER PRIMARY KEY,
    workspace_id           TEXT NOT NULL UNIQUE,
    path                   TEXT NOT NULL,
    display_name           TEXT NOT NULL,
    last_opened_at         INTEGER NOT NULL,
    last_focused_window_id INTEGER,
    is_pinned              INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_workspaces_last_opened ON workspaces (last_opened_at DESC);
CREATE INDEX idx_workspaces_path        ON workspaces (path);
```

Migration: `011_workspaces.sql` (numbered after ADR-009's
`010_layout_state.sql`).

Restore at launch: each persisted window's `last_focused_window_id`
joins to the workspace it had focused at quit; that workspace is
restored as the active workspace in the corresponding window. If the
path no longer exists, the window starts in the no-workspace branch
and the sidebar shows the "workspace not found" banner (failure modes
section). The full recents list is loaded into the switcher regardless.

## Promote-folder-to-workspace

A user opens a one-off `.html` from Finder. No workspace is active. The
preview pane (ADR-009) renders the document under default-deny — no
subresource resolution. The affordance:

1. The preview footer's `PreviewStatusChip` (ADR-009 component) renders
   a new chip variant: `this file's folder is not open as a workspace ·  open as workspace`.
2. Clicking the action promotes the buffer's parent directory: the
   directory is added to the registry, the window's active workspace
   is set to the new entry, the indexer starts on a background thread,
   and the preview re-renders with `writ-workspace://` resolution
   available.
3. The currently-open buffer **stays open**. Its tab gains a workspace
   badge (small left-aligned glyph next to filename). Other tabs in
   the window inside the same directory also gain the badge
   automatically.
4. The active layout is preserved; the re-render is triggered as if
   the user pressed `Cmd+R`, since the policy state effectively
   changed.

## Outside-workspace behavior

A file opened from disk without a workspace continues to function:

- Preview renders under default-deny CSP (ADR-009).
- `writ-workspace://` returns 404 for any request, since the active-
  workspace lookup is empty. Subresources fail silently; the rendered
  document is unstyled — the correct default when there is no
  workspace to resolve against.
- The user can promote-to-workspace, or open the file from inside an
  existing workspace.
- IDE Active and History panels continue to function unchanged.

## Workspace close

The switcher's `Close workspace` action (or removal of the last
anchored tab):

1. If any anchored tab is **dirty**, a confirm appears:
   `closing workspace "<name>" leaves N unsaved buffers · keep open · save and close · close anyway`.
   - `keep open` cancels.
   - `save and close` saves dirty tabs via the existing save path
     (which respects autosave and the watcher's IgnoreSet) then closes.
   - `close anyway` proceeds without saving; tabs remain dirty but
     lose workspace context.
2. The workspace is removed from the window's active-workspace store
   (window enters no-workspace branch).
3. **Tabs remain open** in their windows. They continue to be
   editable. The workspace badge is removed. Subsequent
   `writ-workspace://` requests from these tabs' previews return 404
   until the user promotes the parent again.
4. The workspace is **not** removed from the app-global registry if
   any other window has it focused. The registry entry is removed only
   when no window has it focused.
5. The file index transitions to paused. After 5 minutes of paused
   state with no re-focus, the index rows are evicted; the central DB
   rows survive so re-open is a re-walk, not a re-index.
6. The `workspaces` row's `last_opened_at` updates (close is also a
   use).

## Trust boundary aspect

Workspaces are addressable as trust boundaries. ADR-011 defines the
trust state machine, pin records, per-workspace "trust all" scope, and
the trust dashboard. This ADR commits **only**:

- The `WorkspaceId` type is stable and referenceable from a future pin
  record. ADR-011's `preview_pins` table will hold a `workspace_id`
  foreign key (nullable, so a pin can also be per-file).
- `.writ/pins.toml` is reserved as a path inside the workspace
  directory; v1 does not create or read this file. ADR-011 populates
  it with shareable pin records that survive across machines (per-
  machine pins live in the central DB).
- The protocol handler's allowlist (C) is **not** a trust surface. It
  is a structural constraint on what the protocol can ever serve. What
  the document can *do* with what is served is the CSP, owned by
  ADR-011.

Boundary: ADR-010 owns *where files come from* (workspace root,
allowlist, traversal guard); ADR-011 owns *what documents are allowed
to do* (CSP, script execution, network egress, pins).

## Performance budgets

Asserted in CI per ADR-009's methodology (perf integration tests, p95
over repeat-count, per-platform tables, no historical-baseline ratios).

| Metric                                          | macOS    | Linux    | Windows  |
|-------------------------------------------------|----------|----------|----------|
| Workspace open, 10k files (initial index)       | < 2 s    | < 2 s    | < 2.5 s  |
| Workspace open, 50k files                       | < 8 s    | < 8 s    | < 10 s   |
| Workspace switch (already-indexed)              | < 200 ms | < 200 ms | < 250 ms |
| `writ-workspace://` subresource (cache miss)    | < 20 ms p95 | < 20 ms p95 | < 25 ms p95 |
| `writ-workspace://` subresource (cache hit)     | < 5 ms p95 | < 5 ms p95 | < 6 ms p95 |
| File index incremental update on save           | < 50 ms  | < 50 ms  | < 60 ms  |
| Path-traversal check (per request)              | O(d) where d = canonical-path depth; < 200 µs for d ≤ 32 |
| Workspace close                                 | < 100 ms | < 100 ms | < 120 ms |
| Workspace recents fetch (10 entries)            | < 10 ms  | < 10 ms  | < 12 ms  |

Subresource cache: in-memory LRU keyed by
`(workspace_id, relative_path, mtime)`. Invalidated by the file
watcher's change event for any cached entry. Ceiling: 32 MB per
workspace, evicted LRU. Subresources > 4 MB are streamed, not cached.

Idle memory ceilings (no active subresource fetches):

| Platform | Idle ceiling | Recycle trigger              |
|----------|--------------|------------------------------|
| macOS    | 40 MB        | > 80 MB resident or 30 min idle |
| Linux    | 50 MB        | > 100 MB resident or 30 min idle |
| Windows  | 60 MB        | > 120 MB working set or 30 min idle |

Recycle = pause watcher, drop subresource cache, evict in-memory file
index (central DB rows remain). Re-focus rehydrates from the central
DB at the "workspace switch" budget.

Per-PR delta check (same methodology as ADR-009): CI captures the
workspace-open number and compares against the previous merged
commit's number for the same test on the same platform. A jump > +10%
fails the PR.

## Failure modes

UX failure modes only. Security failure modes (CSP violation, escape
attempts) are owned by ADR-011.

- **Workspace path no longer exists at launch.** The user deleted or
  moved the directory. Window enters no-workspace branch; sidebar
  shows: `workspace "<name>" not found · remove from recent · relocate…`.
  `relocate…` opens the OS folder picker; if the user picks a new
  path, the `workspaces` row's `path` and `workspace_id` update. No
  crash; the banner is the only signal.
- **Workspace on an unreachable network share.** Subresource fetches
  time out; the protocol returns 503; preview shows
  `workspace "<name>" is unreachable · retry`. File watcher pauses; on
  retry a spot-check verifies the share. IDE Active and History remain
  functional.
- **Malformed `.writ/config.toml`.** Parser fails; file ignored
  (fallback to global config); warning logged; workspace status chip
  shows `config invalid` (clickable, opens the file with the parser
  error annotated). Workspace still opens.
- **`.writ/config.toml` contains a denylisted key.** Key dropped; the
  rest of the config applies; warning logged; chip shows
  `config has ignored keys`. Workspace opens.
- **Hostile symlink** (e.g., `<root>/foo.css` → `/etc/passwd`). The
  canonicalize step resolves the target; the prefix-check fails;
  protocol returns 403 with the symlink path and resolved target
  logged. The document's `<link>` simply fails to load.
- **Two windows share the same workspace.** Supported per B2. Both
  windows' active-workspace stores point at the same `WorkspaceId`.
  Subresource requests dedupe at the cache (keyed by `workspace_id`,
  not window). File-index incremental updates dedupe at the indexer.
  Closing in one window leaves the registry entry for the other.
- **Workspace contains > 50,000 files.** Walker prompts:
  `this workspace has 248,317 files · index anyway?` with `Index`,
  `Name only`, `Cancel`. Choice persists in `.writ/config.toml`'s
  `index.size_limit`.
- **File-watcher flood** (e.g., `node_modules` install). Indexer
  batches in 256-event chunks with 50 ms coalescing. Burst > 10,000
  events in 5 s triggers a burst pause; chip:
  `index paused · large file activity · resume`. Manual resume or
  workspace close-reopen forces a re-walk.
- **Disk full during incremental update.** Upsert fails; logged. Next
  save retries. Five failures in 60 s surfaces:
  `index updates failing · disk space`.
- **Symlink loop** (`a/` → `b/` → `a/`). Walker (`walkdir` or
  equivalent) cycle-detects; second visit to the same canonical path
  is skipped. First pass logs the cycle once; subsequent walks are
  silent.

## Cross-platform parity

| OS      | Filesystem default | Case sensitivity                  | Symlinks                       |
|---------|--------------------|-----------------------------------|--------------------------------|
| macOS   | APFS               | Case-insensitive, case-preserving | POSIX symlinks; firmlinks (system-managed) |
| Linux   | ext4/btrfs/xfs     | Case-sensitive                    | POSIX symlinks                 |
| Windows | NTFS               | Case-insensitive                  | NTFS symlinks + junction points (reparse points) |

Canonicalization: **`std::fs::canonicalize`** for the workspace root
and for every incoming relative-path request. The returned path is
absolute with all symlinks resolved. The prefix check
(`request_canonical.starts_with(root_canonical)`) runs **after**
symlink resolution. Case comparison on macOS/Windows uses Unicode
case-folding (`unicase` or equivalent); Linux uses exact byte
comparison.

The unit corpus exercises:

- `..` segments at every depth.
- Absolute-path requests (`/etc/passwd`, `C:\Windows\System32\drivers\etc\hosts`).
- Symlinks inside the workspace pointing outside.
- Symlinks inside pointing to other internal targets (should resolve
  and serve normally).
- Case-only-different paths on macOS/Windows (`./Foo.CSS` vs
  `./foo.css`).
- Symlink cycles (`a/` → `b/` → `a/`).
- Windows junction points (reparse-point handling identical to symlinks).
- Long paths on Windows (`\\?\` prefix; `MAX_PATH` exceeded).
- UNC paths on Windows (`\\server\share\...`).
- Network mounts on macOS (`/Volumes/...`) and Linux (`/mnt/...`,
  `/media/...`); the unreachable-share failure mode applies.

CI runs the corpus on all three platforms. A platform-specific
divergence in security behavior is a release blocker.

## Consequences

### `writ-core`

New `workspace` module: pure-domain types, `Serialize + Deserialize`
for IPC, no Tauri, no async, no I/O.

```rust
pub struct WorkspaceId(String);

pub struct Workspace {
    pub id: WorkspaceId,
    pub path: PathBuf,
    pub display_name: String,
    pub last_opened_at: u64,
}

pub struct WorkspaceConfig {
    pub preview: WorkspacePreviewConfig,
    pub index: WorkspaceIndexConfig,
}

pub struct WorkspacePreviewConfig { /* Option<T> for each allowlisted key */ }
pub struct WorkspaceIndexConfig    { /* Option<T> for each allowlisted key */ }

pub enum WorkspaceCloseReason {
    UserClosed,
    LastTabClosed,
    PathRemoved,
    AppShutdown,
}

pub enum WorkspaceOpenError {
    NotFound { path: PathBuf },
    NotADirectory { path: PathBuf },
    PermissionDenied { path: PathBuf },
    InvalidConfig { reason: String },
    AlreadyOpen { workspace_id: WorkspaceId },
}
```

`WorkspaceId` is constructed via `blake3` over the canonicalized path
bytes, truncated to 16 hex chars for log readability. Different
canonical paths produce different ids; the same canonical path always
produces the same id. This is the property the future shareable pins
in ADR-011 depend on; sequential database ids would not have it.

### `writ-storage`

- New `workspaces` table per the schema above. Migration
  `011_workspaces.sql`.
- New `workspace_id` column on the existing files table (FTS5).
  Nullable: rows for files not inside any workspace
  (one-off opens) carry `NULL`. Migration `012_workspace_id_on_files.sql`.
- New `workspace_store.rs`: insert / update_last_opened / list_recents
  / find_by_path / remove.
- New `workspace_config_store.rs`: read+write `.writ/config.toml`;
  validate against allowlist; drop denylisted keys with logged
  warnings.
- New `workspace_indexer.rs`: walks the workspace root with `walkdir`
  (cycle detection enabled); emits batched FTS5 upserts via the
  existing connection pool.
- New crate dependencies: `walkdir`, `blake3`. No Tauri.

### `writ-plugin`

**Untouched.** Workspace concept is host-side. Future workspace-aware
plugin capabilities reference `writ-core::workspace` types directly.

### `src-tauri`

- New `workspace/` module:
  - `protocol.rs` — `writ-workspace://` handler; registers via
    `tauri::Builder::register_uri_scheme_protocol`. Looks up active
    workspace for the requesting webview's window; canonicalizes;
    runs the traversal guard; dispatches by extension; returns bytes
    (or 403/404/503).
  - `manager.rs` — `WorkspaceManager`: app-global registry, per-window
    active mapping (so the protocol handler can resolve "which
    workspace does this request's window belong to"), open / close /
    switch operations.
  - `indexer.rs` — orchestrates the off-thread walker (the actual
    walk + FTS5 writes live in `writ-storage`); emits progress events;
    implements burst-pause.
  - `cache.rs` — in-memory LRU subresource cache.
- New `commands/workspace.rs` IPC:
  - `workspace_open(path) -> Result<Workspace, WorkspaceOpenError>`
  - `workspace_close(workspace_id, window_id) -> Result<(), String>`
  - `workspace_switch(workspace_id, window_id) -> Result<Workspace, String>`
  - `workspace_list() -> Vec<Workspace>` (open registry)
  - `workspace_list_recents(limit) -> Vec<Workspace>` (from disk)
  - `workspace_promote(buffer_id) -> Result<Workspace, WorkspaceOpenError>`
  - `workspace_rename(workspace_id, display_name)`
  - `workspace_forget(workspace_id)`
  - `workspace_config_read(workspace_id) -> Result<WorkspaceConfig, String>`
  - `workspace_config_write(workspace_id, config)`
- `AppState` gains `workspace_manager: Arc<WorkspaceManager>` and
  `workspace_cache: Arc<WorkspaceSubresourceCache>`.
- The existing file watcher integrates the active workspace as a
  scope: per-workspace watcher, deduplicated across windows that share
  the same workspace. The IgnoreSet behavior (per CLAUDE.md, "always
  insert filename before writing") is unchanged.
- `tauri.conf.json` declares the `writ-workspace` protocol in the
  app's protocol whitelist. The protocol's CSP allowance is owned by
  ADR-011; this ADR commits only that the protocol is registered.

### Frontend

- `src/stores/global/workspaces.ts` — app-global registry singleton,
  comment marked `// Singleton — app-global, not window-scoped`.
  Exports `getOpenWorkspaces`, `getRecentWorkspaces`, `openWorkspace`,
  `closeWorkspace`, `promoteToWorkspace`.
- `src/stores/window/active-workspace-store.ts` — per-window active
  workspace factory; instantiated inside each `<WindowProvider>` per
  ADR-009. Exports `useActiveWorkspace()`.
- `src/components/Sidebar/WorkspacePanel/`:
  `WorkspacePanel.tsx`, `WorkspaceHeader.tsx`,
  `WorkspaceFileTree.tsx`, `WorkspaceProgressChip.tsx`,
  `WorkspacePanel.css`.
- `src/components/WorkspaceSwitcher/`:
  `WorkspaceSwitcher.tsx`, `WorkspaceSwitcherItem.tsx`,
  `WorkspaceSwitcher.css`.
- `src/components/Sidebar/Sidebar.tsx` mounts `WorkspacePanel` above
  `ActiveSection` and `HistorySection`. The panel renders only when
  the window has an active workspace; the empty-state row renders
  otherwise.
- `src/services/tauri.ts` gains the new workspace bindings.
- `src/services/events.ts` gains: `WorkspaceOpened`, `WorkspaceClosed`,
  `WorkspaceSwitched`, `WorkspaceIndexProgress`,
  `WorkspaceIndexComplete`, `WorkspaceUnreachable`,
  `WorkspacePathRemoved`.
- `src/commands/registry.ts` registers `Open workspace`,
  `Close workspace`, `Switch workspace…`, `Rename workspace`,
  `Forget workspace`.
- `src/keymap/workspace.ts` registers `Cmd+Shift+W`. The
  `Cmd+Shift+O` precedence logic lives in `src/keymap/preview.ts`
  (ADR-009 ownership) with an explicit fallback branch documented
  inline with this ADR's precedence table.
- The promote chip variant lives in `PreviewStatusChip.tsx` (ADR-009).
- The store-layer boundary test introduced in ADR-009 is extended:
  `workspaces.ts` belongs in `global/`, `active-workspace-store.ts` in
  `window/`. Test fails if either is moved.

### Configuration

`WritConfig` gains:

```toml
[workspace]
recents_limit             = 10
auto_open_last            = true
index_size_limit          = 50000
index_content_size_cap_mb = 1
unreachable_retry_seconds = 30
```

Settings UI gains a `Workspace` section with these knobs plus a
read-only recents list with `Forget` action per row.

### Styling

All new CSS uses `var(--writ-font-sans)` per ADR-008. The
typography-tokens regression test (ADR-008) passes. The workspace
header inherits `--writ-color-fg`; the path subtitle uses
`--writ-color-muted`. The empty-state row matches the muted treatment
of `HistorySection`.

### Testing

- **Unit (writ-core):** `WorkspaceId` hash stability; `WorkspaceConfig`
  serialization round-trip; `WorkspaceOpenError` variant exhaustiveness.
- **Unit (writ-storage):** workspace CRUD; `find_by_path` returns the
  same row for the same canonical path; recents query ordered by
  `last_opened_at DESC`; `.writ/config.toml` parser accepts allowlisted
  keys, drops denylisted ones with warning, errors cleanly on malformed
  TOML; FTS column scopes queries by `workspace_id`; walker handles
  cycles and platform-conventional hidden-dir ignore patterns.
- **Integration (src-tauri):** `workspace_open` round-trips; protocol
  handler serves correct MIME for each allowlisted extension; the
  traversal guard rejects every entry in the cross-platform corpus
  (the `..` cases, the symlink-escape cases, the absolute-path cases,
  the cycle case); promote transitions the buffer's active-workspace
  state and triggers a re-render; dirty-tab confirm routes through
  each action; two windows sharing a workspace dedupe file-index
  updates.
- **Frontend:** `WorkspacePanel` renders given a mocked store;
  `WorkspaceSwitcher` lists active/other/recents correctly; empty-state
  row appears when no workspace is active; `Cmd+Shift+W` opens the
  switcher; the promote chip in `PreviewStatusChip` calls
  `workspaceOpen` with the parent path.
- **Performance:** `perf/workspace-open.rs` captures 10k and 50k
  synthetic corpora; per-PR delta check applies.
  `perf/workspace-switch.rs` captures already-indexed switch.
- **Cross-platform:** the protocol-handler corpus runs on macOS, Linux,
  Windows in CI; security-behavior divergence blocks release.

## Rationale (composite)

Folder-as-workspace is the only model that survives. A marker-file
model adds ceremony to the common case (drag a folder, work in it) to
satisfy a phantom "publishable definition" requirement workspaces do
not have. Workspaces are inhabited, not declared. A1 makes the system's
model the user's model.

App-global registry plus per-window active workspace is the only state
shape that maps onto ADR-009's E3 hybrid without contortions. The
registry is genuinely app-global (multiple windows want to see and
reuse the same open-workspaces list); active focus is genuinely
per-window (multi-monitor multi-project is a real workflow). ADR-009's
split was sized to accommodate this; B2 fits exactly.

The strict MIME allowlist is the only forward-secure allowlist. C2
creates a per-workspace CSP-relevant surface that would have to
reconcile with ADR-011's trust model; the simpler answer is "the set
of types is fixed in the binary." C3's denylist is the textbook
anti-pattern that has been wrong every time.

FTS5 reuse is right because the alternative ships two independent
search systems. A separate per-workspace SQLite is a binary artifact in
the source tree with no corresponding gain. An in-memory snapshot
re-implements lookup features SQLite already has. The cost is a
migration; the benefit is one search path across the app.

`Cmd+Shift+W` is the cleanest available binding that does not collide.
The original `Cmd+R` collided with ADR-009's force-render; `Cmd+P`
would conflate with open-file; a leader-key prefix for a single
binding is over-architecture. `Cmd+Shift+W` is unbound today, mnemonic,
and adjacent to `Cmd+W` — a coherent W-cluster.

The `Cmd+Shift+O` precedence (active-renderer-tab wins for detach)
maps to the binding's primary intent: when a user has a preview open
and presses the binding, they want detach 100% of the time. When they
have a `.rs` tab or the welcome screen, they cannot mean detach. The
rule reads off obvious context; the shortcut editor surfaces the
precedence table inline.

The `.writ/` directory is minimal because the central database is the
canonical store for per-user state. The one thing `.writ/` adds is
shareable, source-controlled per-workspace settings (`config.toml`)
and the future-reserved pin store (`pins.toml`, ADR-011). Both are
intentional: a project should be able to commit "Writ defaults to
split-view markdown in this repo" today, and "trust the bundled
`/assets/` directory as a static-site root" once ADR-011 lands. The
denylist keeps trust-relevant settings out of TOML files any
contributor can edit.

Performance budgets are asserted because workspace-open is the gateway
to every preview action. A workspace that takes 12 seconds to open is
a workspace the user does not open. The pre-warmed preview pool
(ADR-009) handles the substrate; the file-index budget here handles
the data.

Cross-platform parity is the only way the security verification corpus
(ADR-011) generalizes. A handler that traversal-guards correctly on
macOS but accepts `..\..\` on Windows because of a path-separator
oversight is a single-platform handler. The corpus runs on the matrix;
divergence is a release blocker, not a known issue.

## Open questions deferred to follow-up ADRs

- **Trust model, pin records, per-workspace trust dashboard, CSP per
  scope** → **ADR-011**. This ADR commits only the identity
  (`WorkspaceId`) and the reserved path (`.writ/pins.toml`).
- **Multi-root workspaces** (one window, multiple roots, à la VS
  Code). Deferred to a future ADR. The current model — one active
  workspace per window — is the v1 commitment. Multi-root introduces
  resolution ambiguity the protocol handler is structurally not ready
  for; a future ADR introduces a `WorkspaceRoot` enum
  (`Single`/`Multi`) with explicit precedence rules.
- **Workspace-aware plugin capabilities.** `writ-plugin` is untouched
  in v1; future plugins that want workspace context get an extension
  point in a follow-up ADR. The point will reuse the types declared in
  `writ-core`.
- **External workspace plugins** (e.g., "Rust workspace" with
  cargo-aware affordances). Future work. The folder-as-workspace
  primitive is the substrate any future flavor plugs into.
- **Workspace-aware autocomplete.** A buffer in a workspace should
  autocomplete sibling-file paths in HTML `href`/`src` and markdown
  image references. Reuses the file index. Not v1; the infrastructure
  ships here, the surface is a follow-up.
- **Project-wide search UI.** A future palette filters by workspace
  with paginated results and highlighted previews, using the index
  this ADR populates. Not v1; the data is here.
- **Git integration.** Workspaces with a `.git/` root surface branch
  and dirty-file state. Not v1. A future ADR commits the surface and
  the read-only operations it supports.
- **`detach_persist` workspace context.** ADR-009 reserved
  `[preview] detach_persist`. When that flag lands, the detached
  window's active workspace is restored alongside; bookkeeping uses
  the `last_focused_window_id` column this ADR introduces. Flag and
  restore logic are not v1; the column is.
- **Workspace inheritance / nested workspaces.** v1 does not support
  nesting. Opening a strict subdirectory of an existing open workspace
  produces a warning chip and opens it as a separate workspace. A
  future ADR commits nested-workspace semantics if the use case turns
  out to be real.
- **Workspace templates** (open a folder as a workspace from a
  predefined template — starter blog, markdown project, static site).
  Deferred. Primitive is ready; templating is a layer above.

## Minor decisions landing in implementation PRs (recorded so the PR author does not re-derive)

- **`WorkspaceId` hash algorithm:** `blake3` over the canonicalized
  path bytes, truncated to 16 hex chars. The alternative ("sequential
  integer ids from the database") is tempting and wrong — sequential
  ids would break the "same path → same id across machines" property
  ADR-011's shareable pins depend on.
- **File-watcher scope sharing:** existing per-app file watcher gains
  a per-workspace scope. When two windows share a workspace, only one
  watcher is active for that workspace (deduplicated at the manager).
  Recorded so the PR does not spin up N watchers for N windows.
- **Cache key:** `(workspace_id, relative_path, mtime)`. The `mtime`
  changes when the file changes, which invalidates the cache row
  automatically. The alternative ("use the watcher's change event to
  invalidate") works but is racier — between the event and the next
  request, the cache may serve stale bytes. The mtime closes the race.
- **Display-name collision on rename:** if the user renames a
  workspace to a name already in use, the rename is accepted (names
  are not unique; `WorkspaceId` is the identity). The switcher
  disambiguates by appending the parent segment to the second
  occurrence (`docs (project-a)` vs `docs (project-b)`). The
  alternative ("reject duplicates") is hostile to the common case of
  two projects both named `docs`.
- **CLI argument behavior:** `writ /path/to/folder` opens the folder
  as the active workspace of the new window. If the argument is a
  file, the file opens normally and the promote-to-workspace chip
  surfaces. The alternative ("CLI args open files, never workspaces")
  is at odds with the `code /path/to/folder` ergonomic the user
  already expects.
