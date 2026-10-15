# ADR-052: writ:// links, an allowlisted grammar that never writes

## Status

Accepted, 2026-10-15. Amends [ADR-031](./031-the-ai-harness-and-what-leaves-the-machine.md) rule 1.1.

## Context

Other apps, agents and MCP answers had no way to point at a file or a line in Writ, and a connected program could change a file only by sending all of it back. A URL scheme is input from any web page or program on the machine.

## Decision

1. **Two verbs.** `writ://open?path=<relative>&line=N` shows a file the notes folder holds; `writ://new?name=<name>` (or no name) opens a new tab. `writ_core::writ_link::parse` is the one reading: the scheme matches without case, verbs and parameter names exactly; an unknown verb or parameter, a repeated parameter, a fragment, an unencoded space or control character, a malformed escape, an absolute path, a backslash, a colon, and an empty, `.` or `..` segment are refused. `+` is a plus sign. Refusals are logged by reason, never with the link.
2. **The folder decides.** `open` joins the path to the notes folder as it stands, resolves it on disk and opens it only when the folder holds a file there (`writ_storage::paths::file_in_folder`), so a symlink out is refused. It then goes the way a forwarded path goes: authorized, parked until the window is ready, shown at its line.
3. **Never writes.** `new` asks the window for a tab named `name`; the file appears on its first keystroke (ADR-028 section 2). Neither verb carries text.
4. **One handler, four routes.** The macOS open-URL event, launch arguments on Windows and Linux, a second launch's hand-over and a request `writ mcp` leaves in `open-requests/` (`OpenRequest.links`) all end in `links::follow`. The scheme is registered by the bundler from `plugins.deep-link.desktop.schemes` (Info.plist, MSI, `.desktop`); `tauri-plugin-deep-link` is not linked, because its desktop setup reads `std::env::args()`, which panics on an argument that is not UTF-8, and U23's instance lock already forwards arguments.
5. **The MCP surface.** `append_note`, `replace_in_note` (exact `old_text` found once, else refused) and `today` are write tools through the guarded write, keeping a version; `open_in_writ` is a read tool that hands Writ an `open` link through `open-requests/`. Their answers carry `open_url`, relative to the folder Writ has open. Files are resources (`writ-folder://file/<path>`) read as `read_note` reads them, and `writ-folder://index` lists every indexed file with its title (frontmatter `title`, else the first heading, else the name Writ shows), tags, resolved links and size, rendered from `writ.db` at each read, capped at 5,000 files, written nowhere and approved as `list_notes`.

## Consequences

- Links work in installed builds on macOS and Windows. A dev build and an AppImage have no scheme registered, and the bundled Linux `.desktop` entry claims the scheme with an `Exec` line that carries no `%u`, so a launcher there may start Writ without the link.
- `fuzz/fuzz_targets/writ_link_parser.rs` and a stable property test hold the grammar's invariants.
