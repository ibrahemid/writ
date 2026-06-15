# ADR-020: Deferred, coalesced FTS reindex on the autosave path

**Status:** Accepted
**Date:** 2026-06-13

## Context

Every autosave fires `save_buffer_content`, which writes the buffer file, stamps `updated_at`, and rebuilds the buffer's FTS row with a `DELETE` + `INSERT` over the full content. On a large or rapidly edited buffer the reindex is the dominant cost on the write path, and it runs on the same cadence as the disk write (every autosave debounce window). Search does not need to be that fresh: a buffer's index trailing the file on disk by a couple of seconds is invisible to a user who searches, while a full reindex inside the typing loop is felt as jank.

The existing rule (`.claude/rules/rust.md`) is "`save_content()` MUST update the FTS index after writing." This ADR introduces a deliberate, bounded exception for the IPC autosave path only, and records the consistency guarantees that keep it safe.

## Decisions

### Split the write from the reindex in `BufferStore`

`save_content` is unchanged and still reindexes inline — the perf-gate round-trip test, `open_from_path`, `save_to_source`, recovery writes, and `rename` all keep their synchronous-index semantics. Two new methods carry the deferred path:

- `save_content_without_index(id, content)` — writes the file and stamps `updated_at`, skipping FTS. Identical durability to `save_content` for the bytes on disk.
- `reindex_buffer(id)` — reads the buffer's current title and on-disk content and refreshes its FTS row. Reading from disk (not from a captured string) means a reindex always reflects the latest persisted content, so coalescing multiple edits into one reindex can never index a stale intermediate.

### Coalesce reindexes per buffer on a 2-second idle timer (adapter)

The `save_buffer_content` command calls `save_content_without_index`, then schedules a reindex through an `FtsScheduler` held in `AppState`. The scheduler keeps a per-buffer generation counter: each schedule bumps the buffer's generation and spawns a task that sleeps 2 s, then reindexes only if its captured generation is still the latest. Rapid edits to one buffer therefore collapse to a single reindex 2 s after the last keystroke, and edits to different buffers never block each other. The generation comparison (`should_reindex`) is a pure function and is unit-tested directly.

Mechanism (timers, async tasks) lives in `src-tauri`; policy (what a fresh index contains) stays in `writ-storage`. This matches the existing split where the frontend rations inbox tab-opens while the watcher stays policy-free.

### Flush pending reindexes on shutdown

A 2-second-deferred reindex lost to a quick quit would leave search stale, and the startup consistency check only removes orphaned FTS rows — it never *adds* missing content rows, so it cannot self-heal a dropped reindex. On `RunEvent::ExitRequested` the app drains every buffer with a pending reindex and reindexes it synchronously before exit. Crash-recovered buffers are already reindexed by `save_content` in `AppState::initialize`, so the deferred path introduces no new crash-consistency gap beyond the one the explicit flush closes.

### Why not debounce inside `save_content` itself

Pushing the timer into the storage layer would make `BufferStore` own an async runtime and a background task set, violating the storage-is-synchronous boundary and forcing every caller (tests, recovery, rename) to reason about deferral. Keeping `save_content` synchronous and adding the deferral in the adapter keeps the storage API honest and leaves the perf-gate's `save_content` measurement meaningful.

## Consequences

- Search results for an actively edited buffer can trail the on-disk content by up to 2 s. This is below the threshold a user notices and is bounded by the shutdown flush.
- The architecture rule "`save_content()` MUST update the FTS index" still holds verbatim — `save_content` is untouched. The deferred path is a distinct, named method pair with its own contract documented here.
- A new `FtsScheduler` is the single place that owns reindex timing, so future tuning (interval, batching) is one module.
