# ADR-045: Panic Strategy and Crash Diagnostics

**Status:** Accepted
**Date:** 2026-10-14

## Context

Release builds aborted on panic, so a bug in a parser fed by a note (pulldown-cmark, harper, the renderers) quit Writ with the last second of typing unsaved, and `recover_poison` could never run. A broken WebView2 or a missing webkit2gtk failed the Tauri builder's `expect` with nothing on screen. The panic hook wrote the panic message, which for a string-slicing panic quotes up to 256 bytes of the note, to the logs folder or to the shared temporary directory. Release logged at debug, kept every daily file and named every note by its full path. Release binaries kept no symbol files.

## Decision

- `panic = "unwind"` in the release profile. `writ_core::preview::render_contained` turns a renderer panic into `RenderError::Internal` with a fixed reason; `render_into_cache` and `chat_render_reply` run through it. The `writ-preview://` handler catches a panic in resolving or serializing and answers a 500 built from parts that cannot fail.
- Locks a panic can poison fail closed where they guard script execution: a poisoned render cache serves every document with scripts off, as a poisoned buffer store already did.
- A builder error, or a panic inside the builder (tao panics when GTK cannot start), goes through the startup-failure dialog as `StartupStage::Window`, whose remedy names WebView2 and webkit2gtk-4.1; a panic is reported without its message. zenity gets `--no-markup`.
- The panic hook (`writ_core::crash`) records location, thread, payload kind, byte length and the first 16 hex characters of the payload's SHA-256, never the text. Every panic gets one log line; the first panic at each location and message digest, up to eight per process, also writes `writ-crash-<time>-<n>.log` to the logs folder only, with the full backtrace (frame addresses included) and the runtime address of `install_panic_handler` as the anchor that gives the load slide.
- Release logs at info; `RUST_LOG` turns debug on. `MAX_LOG_FILES` keeps 14 daily files. Log lines carry file names (`writ_storage::paths::file_name_only`), never full paths. The first-paint mode is a typed enum. The log writer's guard lives in the run loop and is dropped on `RunEvent::Exit`, because the event loop ends with `exit` and drops nothing.
- Release profile: `debug = "line-tables-only"`, `split-debuginfo = "packed"`, `strip = "symbols"`. CI files the dSYMs (by LC_UUID), the PDB (by CodeView GUID and age) and the Linux `.debug` file (by GNU build ID, split with objcopy because a packed `.dwp` loses its skeleton to the strip) into a `debug-symbols-<platform>-<version>` workflow artifact. On a public repository any signed-in user can download a workflow artifact, so private means kept off the release; it expires after 90 days.

## Consequences

- A panic inside a wry or AppKit callback outside the protocol handler still aborts: unwinding cannot cross that boundary. The wry fork pinned for the drag-and-drop panic stays.
- Unwind tables grow the binary; `docs/perf-budgets.md` holds the size budget.
- A crash from a release older than 90 days cannot be symbolicated from CI.
- A Linux machine without the webkit2gtk-4.1 library never reaches `main`: the dynamic loader refuses the binary first, so no dialog can appear.
- Error values that embed a path in their own text still reach warn and error lines.
