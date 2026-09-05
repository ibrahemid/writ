# ADR-017: Command-Line Surface

**Status:** Accepted
**Date:** 2026-06-13

## Context

Developer workflows reach Writ through the global hotkey or double-click. They also need shell integration: piping stdin, opening files from a terminal, opening a project folder from `cd`, and wiring Writ as a tool in Claude Code hooks. This ADR defines the `writ` CLI, its authorization handoff to the running app, and the security boundary that governs it.

## Decisions

### Binary: new `crates/writ-cli` workspace crate

A second `[[bin]]` inside `src-tauri` would link all of Tauri into a tool that only parses arguments and shells out to `open -b`. The binary overhead is unacceptable and it violates the "src-tauri is a thin adapter" rule. A separate workspace crate keeps the crate boundary clean. The lib-within-the-crate pattern (`lib.rs` + `main.rs`) makes unit-testing arg parsing without binary linkage straightforward.

### Amended: the crate links `writ-core` and `writ-storage`

The crate originally depended on no workspace crate, and the note verbs (`links`, `backlinks`, `properties`, `tags`, `new`, `rename`, `trash`) changed that. They read the note index the app writes and they resolve links, and both of those are policy: link resolution has one definition in `writ_core::notes::links` (ADR-034) and the note operations have one in `writ_storage::note_ops`. A CLI that copied either would drift from the window, and a link the command line called broken while the app opened it is the whole failure the single definition exists to prevent.

The Tauri boundary is unchanged: the CLI still links no Tauri and is still testable without an app handle. What it costs is size — `writ-storage` brings SQLite, the trash bindings and the walk crates into a binary that used to be `clap` and `std`. The verbs are worth it; a second binary for them would need the same dependencies and would also need installing.

The sidecar is built on its own (`cargo build -p writ-cli --release --target …`), where cargo unifies in no feature from `src-tauri`, so `writ-cli` names `writ-storage`'s `bundled` feature itself rather than relying on a system libsqlite3 being present on the build host.

### Transport: OS open-files path for files and directories

Every operation that carries a user-controlled path must arrive through the OS open-files mechanism. On macOS, `open -b com.writ.editor <path>` routes through `RunEvent::Opened { urls }`, where each entry is a `file://` URL canonicalized by the OS. The existing handler already calls `startup::authorize_and_canonicalize` and records single-use tokens via `AuthorizedPaths::record_for_open`. No modification to the gate is needed; the OS is the authorization attestor for user intent.

On Linux and Windows, a second launch with file paths is caught by `tauri-plugin-single-instance`, which forwards argv to the running instance through the existing `startup::push_arg_paths_into_pending` + `startup::authorize_and_canonicalize` path. This already handles the multi-file case correctly.

Using `open -a Writ` would break if the user renames the app bundle. Using the bundle identifier (`-b com.writ.editor`) is stable across renames and is already present in `tauri.conf.json`.

### Hostile-caller analysis for any URL scheme

A custom `writ://` URL scheme registered via `CFBundleURLTypes` or `tauri-plugin-deep-link` would be web-reachable: any page in any browser, and any application, can invoke `open writ://…`. This makes it unsuitable for any operation that carries a user-controlled file path, because it broadens the authorization surface from "user ran a terminal command" to "anything that can issue an open URL". Therefore:

- No custom URL scheme is registered for file-open or workspace-open operations.
- The stdin scratch case uses no path at all (see below), so it is safe under any transport.

### Directories: dir-branch in the `Opened` handler

When `RunEvent::Opened` delivers a `file://` URL for a directory (which macOS will when `open -b com.writ.editor <dir>` is passed a directory), the existing handler would pass the directory string to the `PendingOpens` event. The frontend consumer calls `win.tabs.openFile(path)` per entry, which calls `validate_file_for_opening` and rejects directories. The path is never inserted into `pending_opens` in the first place.

Instead: in `RunEvent::Opened`, after canonicalization, separate entries into file paths and directory paths. File paths go into `pending_opens` as before. Directory paths immediately call `set_workspace_root_from_path` and emit `WorkspaceChanged` (which fires the existing `writ://workspace-changed` event the frontend already handles). No new event type is needed. If multiple directories arrive, the last one wins — consistent with the single-root constraint in ADR-016.

### Stdin materialization

The CLI detects stdin is a pipe (`!stdin.is_terminal()`), reads all of stdin, and writes the content to `~/.writ/piped/<uuid>.txt` (the `dirs::home_dir()` path). It then opens that file via `open -b com.writ.editor <path>` exactly like a file open. The app receives the path through `RunEvent::Opened`, authorizes it, and opens a normal file-backed buffer. The optional `--title` flag sets the filename suffix so the tab name is meaningful.

The scratch file lives under a well-known Writ-owned subdirectory. No inbox-id custom scheme is needed because the stdin path is not web-reachable: the CLI binary is the only writer, and it writes to an OS-controlled directory.

### Bundling

The CLI binary is bundled as an `externalBin` sidecar (`src-tauri/binaries/writ-<target-triple>`). Tauri renames sidecar binaries with the target triple at bundle time. The `install_cli` IPC command resolves the sidecar path from the app bundle's Resources directory and creates a symlink at `/usr/local/bin/writ`. If the directory is not writable, the command returns the manual `ln -s` command string as an error detail. The symlink is created by the app, not the installer, to defer the privileged step until the user explicitly requests it from Settings.

> **Correction (2026-07-30).** Two details in the paragraph above are wrong about
> the shipped build, kept here rather than rewritten because the record is dated.
> Tauri strips the target triple at bundle time rather than adding one, so the
> bundle contains a plain `writ` (universal on macOS), and `install_cli` resolves
> it next to the running executable, not from `Resources/`: `resource_dir()`
> points at a sibling folder that does not contain the sidecar
> (`src-tauri/src/commands/cli.rs:46-61`). The correct manual symlink is
> `ln -sf "/Applications/Writ.app/Contents/MacOS/writ" /usr/local/bin/writ`.

### Settings UI

Settings → Files section gains an "Install `writ` command" button. The action routes component → store → `tauri.ts` → `install_cli` IPC, following the layering rule. The result is surfaced as a toast.

### `docs/integrations/claude-code.md`

Documents the `PostToolUse` hook recipe and pipe example for use with Claude Code's `/init` or manual `.claude/settings.json`.

## Consequences

- The origin gate is not weakened: all file-path operations arrive through `RunEvent::Opened` (macOS) or the single-instance argv path (Linux/Windows), which are the existing authorized channels. No new authorization path is introduced.
- Directory opens bypass `pending_opens` by design; the Opened handler is the single branch point.
- The CLI crate has no Tauri dependency and can be tested without an app handle.
- The CLI reads the note index over a read-only connection: it runs no migration and writes no row, and it refuses an index whose schema version is not the one its own build embeds rather than reading through the wrong column layout. Repairing the index stays the app's.
- The verbs, their `--json` documents and their exit codes are recorded in [docs/cli-verbs.md](../cli-verbs.md).
- Bundle verification (sidecar resolution + symlink) requires `cargo tauri build` and is not covered by the four local gates; this is noted in the implementation.
- Multi-root support remains out of scope (ADR-016 constraint).
