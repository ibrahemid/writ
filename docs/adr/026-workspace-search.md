# ADR-026: Workspace Search

**Status:** Accepted
**Date:** 2026-07-24

## Context

Writ can search open buffers and history through the FTS5 index over buffer
content. It cannot search the files under the open workspace folder that were
never opened in Writ, and it cannot search their names. A command-palette
surface that searches commands, settings, file names, and file content in one
list needs an engine for the two workspace halves: find a file by name, and grep
its content on demand.

Three questions decide the engine.

## Decision 1: grep on demand, not a disk-file FTS index

Content search runs a parallel grep across the workspace on each query, rather
than indexing every workspace file into FTS5 alongside buffer content.

Indexing disk files would buy instant results and pay for them with staleness,
a database that grows with the workspace, a reindex storm on every branch switch
or large checkout, and a second source of truth for file content that has to be
reconciled against disk. Grep reads the files that exist at query time, so it is
always correct, and it is the model ripgrep and VS Code already use. The cost is
per-query CPU, which is bounded by the file-size cap, the result cap, and
cancellation (below).

Buffer content keeps its FTS index (`search_buffers`): buffers are Writ's own
data, already in the database, and small in number.

## Decision 2: a per-request `tauri::ipc::Channel` for streaming results

Grep results stream to the UI over a `tauri::ipc::Channel<SearchBatch>` passed
as a command argument. This is the first use of `Channel` in the repo; today all
async Rust to UI traffic goes through the typed `WritEvent` bus and its frontend
`EVENT_MAP`.

A channel is ordered, scoped to the call that created it, and closes when that
call ends. The alternative, a new `WritEvent` variant carrying a generation id,
would broadcast every batch to every listener and force manual demultiplexing by
generation on the frontend. A grep can produce hundreds of batches for one query
and a new query starts before the old one drains; the bus is the wrong shape for
that traffic. `services/tauri.ts` already owns every `@tauri-apps/api` import, so
`Channel` lands there and the boundary rule holds.

## Decision 3: one ignore policy, the union of Writ ignores and gitignore

Name search and content search apply the same ignore policy so a file can never
be findable by name and not by content, or the reverse.

The policy is the union of Writ's default ignores (`node_modules`, `target`,
`dist`, and the rest of `DEFAULT_IGNORES`) and the standard git ignore sources
(`.gitignore`, `.ignore`, global gitignore). Hidden files are included: a dotfile
is exactly the kind of file a developer searches for, and excluding them would
hide `.env.example`, `.github/`, and dotfiles that are the target of the search.
`.git/` is always excluded regardless of the hidden-file rule, since its contents
are never source the user edits.

The authoritative union is the `ignore`-crate walk, used for the initial name
index build, every full rebuild, and the content grep. The name index is then
maintained incrementally from the workspace watcher; that patch path re-checks a
single changed path against the union before adding it. The single-path check
covers Writ defaults, the root `.gitignore`/`.ignore`, and the global gitignore.
It does not read `.gitignore` files nested in subdirectories: the `ignore` crate
exposes per-file matchers, not a single-path query over the full nested stack,
and building the whole stack per watcher event is not worth its cost. A file that
only a nested `.gitignore` excludes can therefore enter the index between full
rebuilds; a rescan event (or reopening the workspace) rebuilds from the
authoritative walk and drops it. The gap is names only, never content, since the
grep always walks with the full nested policy.

## Consequences

- Content results are always current with disk. There is no reindex on branch
  switch and no disk-file rows in the database.
- The name index is an in-memory `WorkspaceIndex` in `AppState`, one relative
  path plus a filename offset per entry, capped at 200,000 files. Past the cap it
  is marked truncated and the UI says so.
- A grep is cancellable: `AppState` holds a `search_generation` counter that each
  content-search call bumps and captures, and the walker stops at the next check
  when a newer query has superseded it. In-flight batches from a stale generation
  are discarded on the frontend by their generation stamp.
- Batches carry a `GrepOutcome` on the final batch (`hit_count`, `files_scanned`,
  `truncated`, `cancelled`) so the UI is honest about caps and never presents a
  capped result set as complete.
- The single-path patch check is a known, bounded inconsistency in name search
  under nested `.gitignore`s, converging on the next full rebuild. Content search
  has no such gap.
