# ADR-019: Open Anything — File-Size Ladder and Binary Hex View

**Status:** Accepted
**Date:** 2026-06-13

## Context

Writ's current `validate_file_for_opening` hard-rejects any file over 10 MiB and any file whose first 8 KiB contains a NUL byte. This means developers cannot open large log files, multi-megabyte data files, or compiled artifacts — even as a read-only view. The binary rejection also gives no diagnostic value.

The goal is a graduated policy: small files get full fidelity; large files get syntax stripped but open successfully; very large files confirm before loading; oversized files are refused with an actionable message; binary files open as a read-only hex view instead of being refused. Every tier is implemented as pure functions in `writ-core` with no I/O, keeping the crate boundary rule intact.

Additionally, `read_buffer_content` currently serialises buffer content to JSON for IPC transfer. For large buffers this doubles peak memory and adds latency. Replacing the return type with `tauri::ipc::Response` (raw bytes) eliminates JSON overhead on the hot path.

## Decisions

### Threshold constants (writ-core)

Four named constants define the ladder:

| Constant | Value | Meaning |
|---|---|---|
| `THRESHOLD_NORMAL_BYTES` | 5 MiB | Full feature set below this |
| `THRESHOLD_LARGE_BYTES` | 50 MiB | Large-file mode below this |
| `THRESHOLD_MAX_BYTES` | 500 MiB | Confirm-before-load below this |
| `BINARY_CHECK_BYTES` | 8 KiB | How much to sniff for NUL bytes |

### FileOpenMode enum (writ-core, pure)

```
Normal            — ≤5 MiB text file; today's behaviour.
LargeFile         — (5, 50] MiB text file; syntax, typography, line-wrap off;
                    autosave debounce 2 s; FTS and snapshot excluded.
LargeFileConfirm  — (50, 500] MiB text file; frontend must confirm before loading.
Binary            — contains NUL in first 8 KiB; hex view, read-only.
Refused           — >500 MiB; hard refusal with a descriptive message.
```

`classify_file(size, is_binary) -> FileOpenMode` is a pure function depending on size and the binary flag only.

### Two-phase open for LargeFileConfirm tier

`open_file` validates + classifies but does **not** read file content for the confirm tier. It returns the metadata: size and mode. The frontend (`bufferRegistry.openFile`) calls `requestConfirm` with the size formatted as a human-readable string and a description of what is disabled, then calls `open_file_confirmed(path)` to perform the actual load. `open_file` itself still hard-refuses files above 500 MiB server-side regardless of any frontend call path.

### Binary files: hex view

The hex dump is generated in `writ-core` (`generate_hex_dump(bytes, max_bytes) -> String`), a pure function. Format: `XXXXXXXX  hh hh … hh hh  |ASCII…|`, 16 bytes per row, offset column 8 hex digits, ASCII gutter replaces non-printable bytes with `.`. Input is capped at `HEX_DUMP_MAX_BYTES` (10 MiB) with a truncation notice appended when clamped.

The hex content is stored in the buffer as ordinary text and loaded into CodeMirror as read-only. The buffer's `source_path` points to the original binary file but `save_to_source` is blocked server-side when the buffer was opened as Binary (read-only check). This prevents the hex text from corrupting the binary source.

Binary buffers are never FTS-indexed and never included in heartbeat snapshots.

### Read-only buffers

`BufferDocument` gains a boolean `read_only` field (default `false`, added to the SQLite schema via migration). The server enforces it in `save_buffer_content` and `save_to_source`: both return an error when `read_only = true`. CodeMirror `EditorState.readOnly` and `EditorView.editable` are the UX layer; the server reject is the correctness guarantee.

`read_only` is persisted so it survives session restore. Mode-tier (Normal vs. LargeFile) is re-derived at load time from the stored `size_bytes` field of the `BufferDocument` — this requires a second new column `size_bytes INTEGER NOT NULL DEFAULT 0`.

### FTS skip trade-off

Large-file (>5 MiB) and binary buffers are excluded from FTS indexing at both write (`save_content`, `open_from_path`) and rebuild time. An excluded buffer returns no results from `search_buffers`. This is the correct trade-off: indexing a 50 MiB log file produces a large FTS shadow with marginal utility and degrades search performance for all buffers. The exclusion is permanent for a given buffer; if the file shrinks to below threshold the user must re-open it.

The FTS skip does **not** produce orphan rows because excluded buffers never receive an FTS insert in the first place. `rebuild_fts` also skips buffers with `size_bytes > THRESHOLD_NORMAL_BYTES`.

### Heartbeat snapshot exclusion

`buffer_store::collect_buffer_contents` skips buffers where the backing file size exceeds `THRESHOLD_LARGE_BYTES`. The 30-second heartbeat snapshot in `lib.rs` and the clean-shutdown snapshot both call this method, so both are bounded automatically. This prevents the periodic snapshot from allocating several hundred MB in RAM when large files are open. There is no consistency risk: snapshot recovery is best-effort; skipping a large file from the snapshot means it is not recovered if the app crashes, which is acceptable given the file still exists on disk.

### IPC fast path: raw bytes for read_buffer_content

`read_buffer_content` is changed to return `tauri::ipc::Response` (raw UTF-8 bytes) instead of a JSON-encoded string. The frontend decodes the `ArrayBuffer` using `new TextDecoder().decode(bytes)`. This eliminates JSON string-escaping overhead, halves memory pressure for large buffers, and removes the double-copy that occurs when the JSON engine re-encodes the string value. The JS `invoke()` call returns an `ArrayBuffer` directly when the command returns `Response`.

### Frontend: large-file mode via compartments

`EditorInstance` gains a `readOnlyCompartment` and a `largeFileModeCompartment`. `createExtensions` accepts a `mode: FileOpenMode` parameter:

- `LargeFile` / `LargeFileConfirm` (post-confirm): language and typography compartments receive empty arrays; `EditorView.lineWrapping` is excluded; autosave debounce uses `LARGE_FILE_AUTOSAVE_DEBOUNCE_MS = 2000`.
- `Binary`: same as LargeFile plus `readOnlyCompartment` gets `[EditorState.readOnly.of(true), EditorView.editable.of(false)]`.

Mode is derived from `(buffer.size_bytes, buffer.read_only)` using the same `classify` pure function mirrored in TypeScript, called on every `loadBuffer` so tab-switch always sees the right mode.

### Status bar chip

When the active buffer is in LargeFile or Binary mode, the status bar shows a chip: "Large file · syntax off" (LargeFile) or "Binary · read-only" (Binary). The chip is driven by a `largeFileMode` signal on the window-scoped editor store, set in `loadBuffer` and cleared on buffer close.

## Consequences

- Files up to 500 MiB can be opened (with confirm dialog). Files that would previously cause a silent OOM are now refused at classification time with an actionable error message.
- Binary files produce a useful hex view rather than a generic rejection.
- The schema gains two columns (`read_only`, `size_bytes`) requiring a migration. Existing rows default to `read_only = false` and `size_bytes = 0`; 0 is treated as Normal tier, preserving existing behaviour.
- Crash recovery does not cover large-file buffers. This is a deliberate trade: large file recovery is not valuable (the source file exists), and including these buffers in snapshots risks OOM in the recovery path.
- `validate_file_for_opening` is removed; callers use `classify_path(path) -> FileClassification` which returns both the mode and the size without reading content.
