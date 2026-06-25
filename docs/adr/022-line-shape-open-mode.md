# ADR-022: Line-shape editor open mode for responsive large-file opens

**Status:** Accepted
**Date:** 2026-06-25

## Context

The editor open mode is tiered by byte size in `writ-core` (`classify_file`): `Normal` (≤ 5 MiB) gets the full CodeMirror feature set; `LargeFile` / `LargeFileConfirm` above that drop syntax highlighting, line wrapping, and typography. The frontend mirrored only that byte threshold when mounting the view.

Byte size is the wrong dimension for one common shape. A minified bundle — a single multi-hundred-kilobyte line of JS/JSON/CSS — is small in bytes and so opened as `Normal`, with `lineWrapping`, the language extension (Lezer parse), and bracket matching all active. CodeMirror's per-line layout and measurement cost is superlinear in line length, so mounting one giant line froze the view thread for seconds while the editor was already small enough to dodge every existing safeguard. This is the "hangs on a long file" report, and the byte tier does not see it.

Component measurement of the open path (`@codemirror/state`, real packages) separated the two regimes:

- Many short lines: `EditorState.create` is 55 ms at 30 MiB, 269 ms at 100 MiB; full-document `toString` adds 37–68 ms. Slow but bounded, and these files are already restricted by the byte tier.
- One pathological line: document construction is ~1 ms, but the DOM layout of that line under wrapping + language is the multi-second freeze.

So the freeze is a line-shape problem, not a byte-count one. Document construction is not the freeze, so it is left alone; the fix is the editor feature set the document is mounted with.

## Decisions

### A frontend-only `LongLines` open mode

`editorModeForContent(buffer, content)` (`src/editor/large-file.ts`) resolves the mode from metadata and the loaded content: read-only → `Binary`; size over the byte threshold → `LargeFile`; otherwise, if any line exceeds `MAX_NORMAL_LINE_LENGTH` (10 000 columns, the long-line tokenization cutoff editors like VS Code use) → `LongLines`; else `Normal`. `hasLongLines` scans with an early exit, so a pathological file is detected at its first long line, and the full scan only runs for buffers already under the byte threshold.

`LongLines` is a frontend-only variant of `FileOpenMode`. The backend tiers stay byte-only and never emit it — the byte classification has no content in hand at `classify_path` time, and plumbing a content-shape flag back through the buffer record and IPC would add a schema dimension for a decision the editor already makes where the content lives. The frontend already derived open mode from buffer metadata at mount; this extends that derivation with the one input (content) the backend lacks. `LongLines` joins `LargeFile` / `LargeFileConfirm` in the restricted feature set (no wrap, no language, no typography, longer autosave debounce) and shows a "Long lines · syntax off" status chip so the restriction is never silent.

### Restrict the feature set, not the open-time publish

All buffers publish `currentText` once on open so the published text always matches the buffer id set immediately after it (the `#97` ordering invariant). The per-keystroke materialization — the actual jank for restricted buffers — stays deferred on the update path (`scheduleRestrictedContentPublish`, ADR-020); the bounded one-time publish on open does not. The responsiveness win comes entirely from the restricted feature set, not from deferring the open publish.

## Consequences

- A small minified file opens responsively, in restricted mode, instead of freezing the editor. The trade is syntax highlighting on files whose single-line shape makes highlighting both expensive and near-useless — the same trade every major editor makes, and it is surfaced in the status bar.
- The line-shape decision is a pure, unit-tested function; the escaped bug (byte-only classification) is covered at that layer.
- The byte tiers and the confirm-dialog gate for 50–500 MiB files are unchanged.
