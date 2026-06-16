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
│  │  │  (pure domain)     │  │   │  │  (editor, sidebar,   │ │  │
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
│  │  ┌────────▼───────────┐  │   │  │      Services        │ │  │
│  │  │   writ-plugin      │  │   │  │   tauri.ts (IPC)     │ │  │
│  │  │  extension API     │  │   │  │   typed commands     │ │  │
│  │  └────────┬───────────┘  │   │  └──────────┬───────────┘ │  │
│  │           │              │   │             │             │  │
│  │  ┌────────▼───────────┐  │◄──┼─────────────┘             │  │
│  │  │     src-tauri      │  │   │         IPC Bridge        │  │
│  │  │  (thin adapter)    │  │──►│   invoke() / events       │  │
│  │  │  commands, events  │  │   │                           │  │
│  │  └────────────────────┘  │   └───────────────────────────┘  │
│  └──────────────────────────┘                                   │
└─────────────────────────────────────────────────────────────────┘
```

## Crate Responsibilities

### writ-core
Pure domain logic. No I/O, no framework imports, no async runtime. Contains:
- Buffer model: open/close lifecycle, dirty state, cursor positions
- Config schema: typed structs for user preferences, keybindings, theme tokens
- Domain events: `BufferOpened`, `BufferSaved`, `ConfigChanged`, etc.
- Conflict policy: last-write-wins vs. prompt-on-conflict resolution logic

The constraint is absolute: `writ-core` must compile with no external dependencies beyond
`serde` and `thiserror`. If any Tauri type ever appears here, the workspace will fail to build.

### writ-storage
All persistence. Depends on `writ-core` for domain types, but not on Tauri. Contains:
- SQLite layer via `rusqlite` with WAL mode enabled
- FTS5 full-text search index over buffer content
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
independently of published extension contracts.

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
- **IPC** layer carries typed JSON. Command names and payload shapes are the contract; breaking
  changes require updating both sides atomically.

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
