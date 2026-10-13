# ADR-019: Open Anything — File-Size Ladder and Binary Hex View

**Status:** Accepted; amended 2026-10-13 (every open has a typed answer, the confirm states the size before any read, a reopened tab keeps its mode, tabs restored at launch are measured before any read, a read measures the file first, editor settings and a file's language)
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

### Amended: every open has an answer

The confirm tier no longer travels as a `__CONFIRM_REQUIRED__` error string that the frontend split on `:`. That split broke on a Windows drive colon (`C:\Users\…`, `\\?\C:\…`) and on a file named `10:30.log`: the frontend read the path as `C` and the size as `0 B`. `open_file` answers the tier as data instead: `FileOpenResult { mode: LargeFileConfirm, size_bytes, doc: None }`, and `Refused` likewise (`doc: None`, the size it was refused at). The path the person asked for is the path `open_file_confirmed` reads, untouched.

A refusal is worded in the frontend (`src/lib/open-failure.ts`), through the one byte formatter, so the screen never shows MiB: "it is 600 MB, and Writ opens files up to 524 MB." A size that rounds to the limit is not stated: "it is over the 524 MB limit." Read failures keep their `ERR_READ_*` codes, so an unreadable file says why; an error with no code reads "Writ could not read it." A path in no folder Writ opens from on its own, with no gesture granting it, answers `ERR_OPEN_NOT_AUTHORIZED`: "open it with File > Open File, or drop it onto the window." It answers the same whether or not a file is there, so the answer never says what is on disk outside those folders. A path inside one of them with nothing at it answers `ERR_OPEN_FILE_MISSING`: "it does not exist." A link written before its note is the everyday case, and File > Open File cannot open a file that is not there. Containment is decided on the path resolved as far as it exists (`resolve_for_containment`), so a `..` or a link out of a folder answers as a path outside it does.

`bufferRegistry.openFile` resolves one of `opened | declined | refused { reason } | not-downloaded`, and one store-level reporter (`src/stores/global/open-report.ts`) speaks for every caller: a refusal names the file and the reason, several refusals collapse into one line, and a decline is silent, since Cancel is the person's answer and not a failure.

The confirm is its own dialog (`LargeFileOpen`). It states the name and the size before anything is read, and after Open it shows progress until the editor shows the text. The progress is indeterminate: `open_file_confirmed` reads the file in one synchronous call, so there is no byte count to report until the read moves off the main thread. The dialog yields a paint before that call so the progress state is on screen while it runs.

A file that arrives while nobody is looking (an inbox file, a finished download) asks the same question; nothing over 50 MiB opens without a yes. A question asked while another one or its progress is up waits its turn, so only the person answers a question.

Today's File and reopening a closed file from History answer the same way. `todays_note` returns the dated file's path with its open answer. `restore_buffer` measures the closed file with a stat, since a read would fetch a sync placeholder's bytes, and answers the question or the refusal without reopening anything; `restore_buffer_confirmed` reopens it after Open and measures it again.

### Amended: a reopened tab keeps its mode

Opening a file that already has a tab answers with the mode Rust reports for it, and the confirm tier becomes `LargeFile` once open (`FileOpenMode::once_open`). The frontend keeps the mode each open reported, so returning to the tab never asks again and never shows a 60 MB file with syntax on. `open_file` answers a file in the confirm tier that a tab shows with that tab, so a path that spells the file another way (a link, `/var` for `/private/var`) does not ask again. That answer is the tab's row and nothing more: it reads and follows nothing, as an open of the tab's own path, which the frontend answers without Rust, costs nothing. A file a tab shows that grew past 500 MiB is still refused.

### Amended: tabs restored at launch are measured before any read

`list_active_buffers` measures each restored tab's file with a stat, taken outside the store's lock, and records a size that moved on the row (`BufferStore::record_size`). That write leaves `updated_at` alone: a stat is not a read, and the stamp records when Writ last learned what the file holds. The editor's tier and the index read the size off the row, so a file that grew while Writ was closed is treated at the size it has now. The launch's restore of the unsaved-text journal measures the file itself and reads none over 50 MiB ([ADR-044](./044-unsaved-text-journal.md) §4). The reopen from History records the size it measured on the row the same way. A path with no file behind it is listed at no size, so nothing waits on the size it had: its tab is read when shown, and the read says at once that the file is gone. The row keeps the size on record.

A restored tab whose file is now over 50 MiB stays unread (`src/stores/global/unread-tabs.ts`), decided once from the rows the launch listed. When the tab is shown it answers as reopening a closed file from History does: `restore_buffer` measures the file again and the question states the size, the read waits for Open, and `restore_buffer_confirmed` measures once more. The tab was never closed, so nothing is restored, its stamp does not move, and it is not followed again: it has been followed since the launch listed it. Cancel leaves the tab unread, and the editor says the file is not open, with Open… (the question again) and Close. A file over 500 MiB stays unread as refused: the editor shows the reason in the file's place, with Try again, which measures again. An open of the tab's path from the tree, the palette or a drop is the tab's open, with the same question. A path that spells the file another way reaches Rust, which answers with the tab's row and reads nothing, and the frontend sends that answer to the tab's open too; Today's File does the same. Until that open answers, the frontend reads none of the tab's text: `readContent` refuses the editor, the reload after a change outside Writ and Save a Copy alike. Closing the tab reads nothing either: its unread state clears only once it has left the open tabs, Close All Tabs included. Two Rust paths read the file of any open tab, an unread one included, and are unchanged here: the open-file watcher hashes the file on each change, and on a volume with no file id (FAT, exFAT, some SMB shares) the identity read hashes it when the tab is followed.

### Amended: a read measures the file first

`read_buffer_content` measures the file with a stat before it reads it (`BufferStore::read_content`). A file past 500 MiB is not read. The answer is `ERR_OPEN_TOO_LARGE`, the code an open of such a file answers with, and the size found is recorded on the row, leaving `updated_at` and the record of what the file held as they were. A file reaches that size after its open measured it: a restored tab that grew before it was shown, or an open log that kept growing. The frontend takes the tab into the unread set as refused, with the size from the row: the editor drops the view and says why in its place, with Try again, which measures again and asks in the confirm tier. The reload after a change outside Writ is the same read, so it ends on the same pane rather than reading the file. The index picks files by the row's size, so the recorded size keeps it off the file too. The heartbeat reads no file ([ADR-044](./044-unsaved-text-journal.md) §6).

The confirm tier stays the frontend's question, asked from the row's size: a file between 50 and 500 MiB is read, and one that grew into that tier after its tab last measured it is read without the question.

### Binary files: hex view

The hex dump is generated in `writ-core` (`generate_hex_dump(bytes, max_bytes) -> String`), a pure function. Format: `XXXXXXXX  hh hh … hh hh  |ASCII…|`, 16 bytes per row, offset column 8 hex digits, ASCII gutter replaces non-printable bytes with `.`. Input is capped at `HEX_DUMP_MAX_BYTES` (10 MiB) with a truncation notice appended when clamped.

The hex content is stored in the buffer as ordinary text and loaded into CodeMirror as read-only. The buffer's `source_path` points to the original binary file but `save_to_source` is blocked server-side when the buffer was opened as Binary (read-only check). This prevents the hex text from corrupting the binary source.

Binary buffers are never FTS-indexed, and nothing in them reaches the unsaved-text journal: their view is read-only ([ADR-044](./044-unsaved-text-journal.md) §2).

### Read-only buffers

`BufferDocument` gains a boolean `read_only` field (default `false`, added to the SQLite schema via migration). The server enforces it in `save_buffer_content` and `save_to_source`: both return an error when `read_only = true`. CodeMirror `EditorState.readOnly` and `EditorView.editable` are the UX layer; the server reject is the correctness guarantee.

`read_only` is persisted so it survives session restore. Mode-tier (Normal vs. LargeFile) is re-derived at load time from the stored `size_bytes` field of the `BufferDocument` — this requires a second new column `size_bytes INTEGER NOT NULL DEFAULT 0`.

### FTS skip trade-off

Large-file (>5 MiB) and binary buffers are excluded from FTS indexing at both write (`save_content`, `open_from_path`) and rebuild time. An excluded buffer returns no results from `search_buffers`. This is the correct trade-off: indexing a 50 MiB log file produces a large FTS shadow with marginal utility and degrades search performance for all buffers. The exclusion is permanent for a given buffer; if the file shrinks to below threshold the user must re-open it.

The FTS skip does **not** produce orphan rows because excluded buffers never receive an FTS insert in the first place. `rebuild_fts` also skips buffers with `size_bytes > THRESHOLD_NORMAL_BYTES`.

### Heartbeat snapshot exclusion

[ADR-044](./044-unsaved-text-journal.md) retired the heartbeat snapshot and `collect_buffer_contents`. The heartbeat now runs every 120 s and reads no file, and a note's text reaches `writ.db` only when no file holds it, in the unsaved-text journal. The paragraph below records the decision as it was made.

`buffer_store::collect_buffer_contents` skips buffers where the backing file size exceeds `THRESHOLD_LARGE_BYTES`. The 30-second heartbeat snapshot in `lib.rs` and the clean-shutdown snapshot both call this method, so both are bounded automatically. This prevents the periodic snapshot from allocating several hundred MB in RAM when large files are open. There is no consistency risk: snapshot recovery is best-effort; skipping a large file from the snapshot means it is not recovered if the app crashes, which is acceptable given the file still exists on disk.

### IPC fast path: raw bytes for read_buffer_content

`read_buffer_content` is changed to return `tauri::ipc::Response` (raw UTF-8 bytes) instead of a JSON-encoded string. The frontend decodes the `ArrayBuffer` using `new TextDecoder().decode(bytes)`. This eliminates JSON string-escaping overhead, halves memory pressure for large buffers, and removes the double-copy that occurs when the JSON engine re-encodes the string value. The JS `invoke()` call returns an `ArrayBuffer` directly when the command returns `Response`.

### Frontend: large-file mode via compartments

`EditorInstance` gains a `readOnlyCompartment` and a `largeFileModeCompartment`. `createExtensions` accepts a `mode: FileOpenMode` parameter:

- `LargeFile` / `LargeFileConfirm` (post-confirm): language and typography compartments receive empty arrays; `EditorView.lineWrapping` is excluded; autosave debounce uses `LARGE_FILE_AUTOSAVE_DEBOUNCE_MS = 2000`.
- `Binary`: same as LargeFile plus `readOnlyCompartment` gets `[EditorState.readOnly.of(true), EditorView.editable.of(false)]`.

Mode is derived from `(buffer.size_bytes, buffer.read_only)` using the same `classify` pure function mirrored in TypeScript, called on every `loadBuffer` so tab-switch always sees the right mode.

### Amended: editor settings and a file's language

`editor.word_wrap` and `editor.tab_size` each have a compartment in `EditorInstance` (`wrapCompartment`, `indentationCompartment`), reconfigured on every open editor when the setting changes. Wrapping applies to Normal files only, as before; tab size sets both `tabSize` and `indentUnit`. `editor.font_family` and `editor.markdown_typography` are removed: nothing read either. The faces are the type pairing's (ADR-030), and Inline | Source decides how Markdown is drawn. A config file that still carries them loads, and the next save leaves them out.

A file's language is decided from its name. A fence's info string names the language of the fence only, so a `.txt` holding a Python fence stays plain text. Only a name with no extension lets the text speak: a shebang on the first line, or code the content scorer recognises once the file holds enough of it. Change Language in the palette records a language for one file in `buffers.language` (`set_buffer_language`); `plaintext` is a choice too. The column held a language guessed from the extension until now, so migration 044 clears it, and the open path no longer writes one.

Grammars come from `@codemirror/language-data` through `LanguageDescription` (`src/editor/languages.ts`) and load the first time a file needs one, so the main chunk carries none. A name Writ's own map does not list is looked up in that table: a whole name it knows (`Dockerfile`, `PKGBUILD`) takes its language, and otherwise the extension does, in any case. A name that says plain text takes none, the table's `CMakeLists.txt` included, and neither does an extension the table gives to more than one language (`.m`) or reads as a language most such files are not in (`.cfg`, `.in`). Markdown (with the GitHub extensions and every fence highlighted in its language) and JavaScript (with JSX) are Writ's own descriptions over the same table entries. The restricted tiers ignore a chosen language: syntax stays off above 5 MiB.

### Status bar chip

When the active buffer is in LargeFile or Binary mode, the status bar shows a chip: "Large file · syntax off" (LargeFile) or "Binary · read-only" (Binary). The chip is driven by a `largeFileMode` signal on the window-scoped editor store, set in `loadBuffer` and cleared on buffer close.

## Consequences

- Files up to 500 MiB can be opened (with confirm dialog). Files that would previously cause a silent OOM are now refused at classification time with an actionable error message.
- Binary files produce a useful hex view rather than a generic rejection.
- The schema gains two columns (`read_only`, `size_bytes`) requiring a migration. Existing rows default to `read_only = false` and `size_bytes = 0`; 0 is treated as Normal tier, preserving existing behaviour.
- Crash recovery does not cover large-file buffers. This is a deliberate trade: large file recovery is not valuable (the source file exists), and including these buffers in snapshots risks OOM in the recovery path.
- `validate_file_for_opening` is removed; callers use `classify_path(path) -> FileClassification` which returns both the mode and the size without reading content.
