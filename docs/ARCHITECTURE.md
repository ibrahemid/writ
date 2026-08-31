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
- Conflict policy: last-write-wins vs. prompt-on-conflict resolution logic
- Recovery policy: snapshot retention and which buffers to restore after an unclean launch
- File classification: the open-mode ladder in `file_ops`, which decides whether a path opens
  normally, as a large file, or is refused

`writ-core` is not I/O-free. `file_ops` makes bounded `std::fs` probes (`metadata` for size, a
head-of-file read to sniff for NUL bytes, `canonicalize` on argv paths) because classification
cannot be decided without them. Reading and writing file contents is `writ-storage`'s job and
stays there.

The enforced constraint is the dependency direction: `writ-core` depends on no other
workspace crate, and if any Tauri type appears here the workspace fails to build. Its
external dependencies are `serde`, `serde_json`, `uuid`, `chrono`, `tracing`, `thiserror`,
`sha2` and `url`.

### writ-storage
All persistence. Depends on `writ-core` for domain types, but not on Tauri. Contains:
- SQLite layer via `rusqlite` with WAL mode enabled
- FTS5 search index over the notes folder, keyed by canonical file path (ADR-028)
- File I/O: reading and writing files to disk with atomic renames
- TOML parsing for config files using `toml` crate
- Crash recovery (wired): `recovery::snapshot` writes periodic session snapshots embedding
  active-buffer contents, prunes to a bounded count on every write, and resolves which
  buffers to restore after an unclean launch; `recovery::dirty_shutdown` detects the unclean
  launch via the `session_snapshots` table. `AppState` runs detection and restore on init
  (before the watcher starts and before empty-scratch reclaim), a background thread writes
  an unclean heartbeat snapshot every 30 s, and `ExitRequested` writes a clean snapshot.
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
workspace directory, or piped text written under `~/.writ/piped/`) and hands it to the app:
by bundle id on macOS, by the sibling app binary elsewhere, with the OS default handler as
the fallback. Depends on no workspace crate and has no Tauri dependency, so it is testable
without an app handle. Bundled as a Tauri sidecar that ships beside the app executable.
See [ADR-017](./adr/017-command-line-surface.md).

### src-tauri
The only crate that imports `tauri`. Thin adapter responsibilities only:
- Registers Tauri commands that delegate immediately to `writ-core` or `writ-storage`
- Translates Tauri events to domain events and vice versa
- Manages application lifecycle (setup, teardown, window configuration)
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
