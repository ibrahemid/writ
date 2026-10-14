# ADR-047: Commands never block the main thread

## Status

Accepted, 2026-10-14. Amends [ADR-019](./019-open-anything-ladder.md): the confirmed open reads off the main thread. Amends [ADR-044](./044-unsaved-text-journal.md): a save to a stalled share holds its note's queue and a worker, not the store or the main thread.

## Context

Tauri runs a command that is not `async` inline on the main thread. About 110 of Writ's commands were, so opening a 300 MB log after the size prompt, saving it, rendering a long note, ranking a 200,000-file folder and the administrator password dialog of the `writ` command install each held the window: no drawing, no menus, no other command. The main thread also served the `writ-preview` protocol and was, by accident, what kept two saves of one note in order (TS-06, EP-14).

## Decision

1. **IO-bound commands are `async` and hand their work to the blocking pool** (`commands::offload::run_blocking`). The main thread only dispatches them. The `_inner` functions stay synchronous; frontend command names, arguments and answers are unchanged. A command whose work panics answers with its own failure code. Commands that only touch rows, settings, or window, menu and shortcut state stay synchronous.
2. **One queue per note** (`writ_core::serial::KeyedQueue`, `AppState::buffer_writes`). Every command that writes a note's file, or reads it and records what it holds (save, restore, Save As, the change bar's answers, read, disk state, reopen, rename, close, delete, a version restore, a chat edit), takes a place in that note's queue as it begins and runs once the commands before it have finished. Renders of one buffer queue the same way in the render cache.
3. **The store lock is never held across a file's bytes.** A save is planned under the store (`BufferStore::plan_source_write`), written without it (`SourceWrite::land`, still under the data folder's `write.lock`), and settled under it. Reads, the open, the reopen resync and the deferred reindex read outside the store and take it back to record. The note's turn is taken before the store lock, never under it.
4. **`writ-preview` is an asynchronous protocol.** Requests resolve on the blocking pool and always respond, a panic included. A rendered document is shared through `Arc`, and a note's wikilinks are resolved in one lookup before it renders.
5. **No file is read to be skipped.** A reindex checks `read_only` and the size before reading, a save schedules no reindex for the large tiers, an open reads and hashes the file once, and a disk-state check reads once.
6. **Startup maintenance runs after the window shows**, behind the store and the notes index's writer; pickers await a oneshot rather than parking a worker.
7. **A shell budget** (`docs/perf-budgets.md`, `tests/shell_budget.rs`) dispatches the listed commands through the mock runtime and fails when one holds the dispatching thread.

## Consequences

- Commands now run beside each other. Order holds within one note's queue and nowhere else; the write guard is what answers a write from elsewhere.
- A rename reads and hashes its file with the store released (`note_ops::prepare_rename`); only the `fs::rename` and the row write run under it, and the move refuses a name taken in between.
- The JSON body of a large save is still parsed on the main thread before dispatch (EP-13).
