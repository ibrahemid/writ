# Writ Architecture

Writ is a lightweight text editor built with Tauri v2, SolidJS, and CodeMirror 6. The design
prioritizes a minimal binary footprint, compiler-enforced separation between business logic and
framework code, and typed contracts across every layer of the stack.

## System Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                         Tauri Shell                             │
│                                                                 │
│  ┌──────────────────────────┐   ┌───────────────────────────┐  │
│  │      Rust Backend        │   │     SolidJS Frontend       │  │
│  │                          │   │                            │  │
│  │  ┌────────────────────┐  │   │  ┌──────────────────────┐ │  │
│  │  │     writ-core      │  │   │  │     Components       │ │  │
│  │  │  (domain model)    │  │   │  │  (editor, sidebar,   │ │  │
│  │  │  buffer, config,   │  │   │  │   tabs, statusbar)   │ │  │
│  │  │  events, policy    │  │   │  └──────────┬───────────┘ │  │
│  │  └────────┬───────────┘  │   │             │             │  │
│  │           │              │   │  ┌──────────▼───────────┐ │  │
│  │  ┌────────▼───────────┐  │   │  │       Stores         │ │  │
│  │  │   writ-storage     │  │   │  │  (buffer, config,    │ │  │
│  │  │  SQLite, file I/O  │  │   │  │   session, search)   │ │  │
│  │  │  TOML, FTS5        │  │   │  └──────────┬───────────┘ │  │
│  │  └────────┬───────────┘  │   │             │             │  │
│  │           │              │   │  ┌──────────▼───────────┐ │  │
│  │           │              │   │  │      Services        │ │  │
│  │           │              │   │  │   tauri.ts (IPC)     │ │  │
│  │           │              │   │  │   typed commands     │ │  │
│  │           │              │   │  └──────────┬───────────┘ │  │
│  │           │              │   │             │             │  │
│  │  ┌────────▼───────────┐  │◄──┼─────────────┘             │  │
│  │  │     src-tauri      │  │   │         IPC Bridge        │  │
│  │  │  (thin adapter)    │  │──►│   invoke() / events       │  │
│  │  │  commands, events  │  │   │                           │  │
│  │  └────────────────────┘  │   └───────────────────────────┘  │
│  └──────────────────────────┘                                   │
└─────────────────────────────────────────────────────────────────┘
```

The diagram shows the runtime path. The full set of cargo dependency edges between
workspace members, which is what the build enforces:

```
src-tauri ─┬─▶ writ-core
           ├─▶ writ-storage ──▶ writ-core
           ├─▶ writ-plugin ───▶ writ-core
           ├─▶ writ-lint ─────▶ writ-core
           └─▶ writ-render        (no workspace dependencies)

writ-cli                          (no workspace dependencies; ships beside the
                                   app binary as a Tauri sidecar, not linked in)
```

`writ-storage`, `writ-plugin` and `writ-lint` are siblings on `writ-core`. None of
them depends on another.

## Crate Responsibilities

### writ-core
Domain model and policy. Zero Tauri, no framework imports, no async runtime. Contains:
- Buffer model: open/close lifecycle, dirty state, cursor positions
- Config schema: typed structs for user preferences, keybindings, theme tokens
- Domain events: `BufferOpened`, `BufferSaved`, `ConfigChanged`, etc.
- Recovery policy: snapshot retention and which buffers to restore after an unclean launch
- File classification: the open-mode ladder in `file_ops`, which decides whether a path opens
  normally, as a large file, or is refused
- Notes-folder policy: the default folder `~/Writ`, the containment rule for paths inside it,
  the title sanitiser shared by the migration, rename and auto-title, and the write guard
  (`notes::guard::decide_save` over a recorded `DiskState`) that decides whether a save may land
  on a file that changed under Writ
- Watcher ignore stamps keyed by canonical absolute path rather than by bare filename, so a save
  of one note cannot suppress a genuine external change to another

The notes-folder policy and the re-keyed stamps arrive with the storage change in release 0.4
([ADR-028](./adr/028-files-are-the-only-copy.md)).

`writ-core` is not I/O-free. `file_ops` makes bounded `std::fs` probes (`metadata` for size, a
head-of-file read to sniff for NUL bytes, `canonicalize` on argv paths) because classification
cannot be decided without them. Reading and writing file contents is `writ-storage`'s job and
stays there.

The enforced constraint is the dependency direction: `writ-core` depends on no other
workspace crate, and if any Tauri type appears here the workspace fails to build. Its
external dependencies are `serde`, `serde_json`, `uuid`, `chrono`, `tracing`, `thiserror`,
`sha2`, `url` and `unicode-segmentation`.

### writ-storage
All persistence. Depends on `writ-core` for domain types, but not on Tauri.

The file on disk is the only copy of a note's text. SQLite holds derived data only: the FTS5
index keyed by canonical path under the notes folder, the index tables created by migration
`040_notes_migration.sql`, per-file history metadata, session and layout state, and tab order.
Deleting `writ.db` costs a reindex and the session layout, and loses no note. The mirror files
under `~/.writ/buffers/` are retired by the same migration, which verifies every mirror against the
file that now holds its text before unlinking it and keeps a rollback copy at
`writ.db.pre-notes-migration`. One copy of text outside a file stays legitimate: the recovery
snapshot, covering text that has not reached a file yet.

Contains:
- SQLite layer via `rusqlite` with WAL mode enabled
- FTS5 search index over the notes folder, keyed by canonical file path (ADR-028)
- The `files`, `links`, `properties`, `tags` and `headings` index tables, created empty by
  migration `040_notes_migration.sql` and keyed by canonical path
- File I/O: reading and writing files to disk with atomic renames. A save is refused with
  `StorageError::SourceChangedOnDisk` when the hash of the bytes on disk differs both from the
  last known hash and from the content being written
- TOML parsing for config files using `toml` crate
- Crash recovery (wired): `recovery::snapshot` writes periodic session snapshots embedding only
  text that has not reached a file, prunes to a bounded count on every write, and resolves which
  buffers to restore after an unclean launch; `recovery::dirty_shutdown` detects the unclean
  launch via the `session_snapshots` table. `AppState` runs detection and restore on init
  (before the watcher starts and before empty-scratch reclaim), a background thread writes
  an unclean heartbeat snapshot on a timer that 0.4 slows from 30 s, and `ExitRequested` writes
  a clean snapshot.
  Retention and resolution policy live in `writ-core::recovery`; mechanism lives here.
  `AppState::initialize` also runs the read-only `ConsistencyChecker` on boot, logging
  orphaned backing files and rows whose content file is missing (repair policy is deferred).

### writ-plugin
Defines the extension boundary. Provides a stable API surface that plugins target. Depends on
`writ-core` types. Isolates the plugin ABI from Tauri internals so the host runtime can evolve
independently of published extension contracts. Also holds the shipped text-transform runtime:
the `TextTransform` trait, the registry, and the built-in and composite transforms
(see [ADR-006](./adr/006-plugin-runtime-v1.md) and [ADR-012](./adr/012-composite-transforms.md)).

### writ-render
Markdown to HTML-fragment core. Turns buffer text into the fragment the preview pane renders,
including fenced-diagram blocks. `pulldown-cmark` is its only non-optional dependency and it
depends on no workspace crate, so it stays callable outside the app; `crate-type` includes
`cdylib` and an optional `wasm` feature compiles it to WebAssembly. `src-tauri`'s preview
renderers are the in-tree consumer.

### writ-lint
Spell check and mechanical-writing rules, wrapping `harper-core` against its curated
in-process dictionary. Depends on `writ-core`. Style and readability rules are off by design:
Writ flags mistakes, not prose taste. Harper reports character offsets and CodeMirror measures
UTF-16 code units, so the crate converts every span before returning.

### writ-cli
The `writ` command line binary. Parses argv and stdin into an open target (a file list, a
workspace directory, or piped text, which becomes a note in the notes folder like any other new
note) and hands it to the app: by bundle id on macOS, by the sibling app binary elsewhere, with
the OS default handler as the fallback. Depends on no workspace crate and has no Tauri
dependency, so it is testable without an app handle. Bundled as a Tauri sidecar that ships
beside the app executable.
See [ADR-017](./adr/017-command-line-surface.md), and
[ADR-028](./adr/028-files-are-the-only-copy.md) section 1 for the piped-text target.

### src-tauri
The only crate that imports `tauri`. Thin adapter responsibilities only:
- Registers Tauri commands that delegate immediately to `writ-core` or `writ-storage`
- Translates Tauri events to domain events and vice versa
- Manages application lifecycle (setup, teardown, window configuration)
- Blesses the notes-folder root as a containment root in
  `src-tauri/src/security/authorized_paths.rs`, so any note inside the folder is writable,
  including one a sync client delivered and the sidebar opened
- Refuses to start when the data directory resolves inside a sync provider's tree, and asserts
  that the database is not inside the notes folder
  ([ADR-028](./adr/028-files-are-the-only-copy.md) section 9)
- No business logic; if logic accumulates here, it must be pushed down into `writ-core`

## Frontend Architecture

```
Components → Stores → Services (tauri.ts) → IPC → Rust commands
```

- **Components** are pure-reactive SolidJS. They read from stores and dispatch actions; they do
  not call `invoke()` directly.
- **Stores** hold derived and authoritative UI state using SolidJS signals and `createStore`.
- **Services / tauri.ts** is the single file that calls `@tauri-apps/api/core` `invoke()`.
  All IPC payloads and responses are typed with generated or hand-maintained TypeScript interfaces
  that mirror the Rust command signatures.
- **Services / events.ts** is the single file that imports `@tauri-apps/api/event`. Every
  `listen` and `emit` goes through it, so the set of live subscriptions is readable in one place.
- **IPC** layer carries typed JSON. Command names and payload shapes are the contract; breaking
  changes require updating both sides atomically.

Two rules keep the DOM out of the reactive graph:

- No `document.querySelector` in components or stores. Reach an element with a ref, or keep the
  state in a store. Test files are exempt.
- No `document.addEventListener` outside `onMount` or `createEffect`, and every listener is
  removed in a matching `onCleanup`. A listener registered at module scope outlives the
  component that wanted it.

### Design tokens

Every colour, radius, type step, shadow and duration is a DTCG token under `design/tokens/`.
`pnpm tokens:build` emits four files, all of them read-only:

| Output | Consumer |
|---|---|
| `src/styles/generated/theme.css` | the app, imported by `src/styles/global.css` after `fonts.css` |
| `src/styles/generated/tokens.ts` | TypeScript that needs a token value or an accent id |
| `src-tauri/assets/generated/preview-tokens.css` | the `writ-preview://` iframe, inlined by `renderers/theme.rs` |
| `site/design-system/generated/tokens.css` | nothing yet: no page imports it, and the demo window declares its own `--writ-*` values in `site/src/styles/writ-window.css` |

A component stylesheet spends `var(--writ-*)`; it never declares one. Three architecture tests
hold the line: `no-literal-color`, `no-raw-radius-or-easing`, and `legacy-aliases`, which carries
the pre-ADR-030 names and fails any file that still reads one. A new token is added to
`design/tokens/` and the outputs regenerated in the same commit, which CI checks by rebuilding
and diffing ([ADR-030](./adr/030-design-system-tokens.md)).

## Design Principles

1. **Compiler-enforced boundaries** — workspace dependency constraints prevent accidental coupling.
   The build is the enforcer, not code review comments.

2. **Policy in core, mechanism in adapter** — `writ-core` decides what should happen; `src-tauri`
   decides how to surface that to Tauri's event loop. Core never imports Tauri; adapter never
   contains business logic.

3. **Typed events end-to-end** — Rust enums serialize to JSON, TypeScript interfaces deserialize
   from JSON. The `tauri.ts` service layer owns the mapping and is the only place where
   `as unknown as T` casts are tolerated.

4. **No speculative complexity** — features are added when needed. The plugin crate exists to
   define a boundary, not to ship a full extension runtime on day one.

## Architecture Decision Records

Individual decisions are documented in [`docs/adr/`](./adr/). Start with
[ADR-001](./adr/001-tauri-over-electron.md) for the top-level shell choice and work forward.
[ADR-028](./adr/028-files-are-the-only-copy.md) is the record for where a note's text lives, and
[ADR-030](./adr/030-design-system-tokens.md) for the token layer.
