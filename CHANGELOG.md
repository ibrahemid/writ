# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.3.0] - 2026-07-30

### Added

- Search everywhere on `Cmd+Shift+F`: one palette over commands, settings, and every buffer, open or from history. With a workspace folder open it also matches file names and greps file contents on each query, streaming results as they arrive. Prefixes route the query: `>` commands, `#` content, `:` go to line.
- `Cmd+click` (`Ctrl` elsewhere) opens `http`, `https`, and `mailto` links from the editor in the default browser, underlining a link only while the modifier is held. A relative link that resolves inside the workspace opens in Writ instead.
- A link clicked in the preview names the host it points to and asks before opening. A scheme outside the allowed set states why it was refused and offers no way to open it.
- A Writ context menu in the editor, replacing the webview's. It offers what fits the moment: spelling corrections on a flagged word, link actions on a link, clipboard verbs, the rewrite actions when text is selected, and a workspace search seeded with the selection. Text fields get their own cut/copy/paste menu.
- Rewrite gained an "improve prompt" action, which rewrites the selection as a clearer instruction for a model and reproduces `{{placeholder}}` tokens untouched.
- Spelling corrections in place: double-click a flagged word to see its suggestions above it and click one to apply, or add that single word to the dictionary. Previously a word could only be fixed in bulk or ignored.
- Commands carry search keywords, so the palette finds them by terms that appear in neither their label nor their description.
- A "Third-party licences" row in Settings opens the bundled `THIRD-PARTY-NOTICES.md` in a buffer.
- A startup failure shows a dialog and writes a report file before the app exits. It previously aborted with no window and no trace of why.

### Changed

- Writ requires macOS 12 or later. The bundle previously declared 10.15, so a 0.2.0 install on macOS 11 or earlier must not update: macOS will refuse to launch this build and the update feed carries no OS check.
- The rewrite actions share a `Rewrite:` prefix and are findable as a group; searching the palette for "rewrite" previously returned only the custom action.
- Consent for a hosted provider is asked when the first rewrite runs, and names the host receiving the text. It was previously reachable only from a notice at the foot of the AI settings section, which left a configured provider failing with no way forward.
- Rewrite failures name the host and offer the setting that fixes them, and a failed rewrite can be retried without re-selecting the text.
- The landing page was rebuilt around sections that render as the page scrolls, and its demo window now runs the search palette. The changelog page lists every release instead of a single list of notes.
- The Linux install script checks the download against `SHA256SUMS.txt`, and against the minisign signature when minisign is on PATH. It installs the CLI as the `writ` command, so piping into `writ` reaches the CLI rather than the app.

### Fixed

- Menu items that opened a group did nothing when clicked and could not be reached by keyboard: Spelling settings, Close All Tabs, and Clear All History.
- Menus no longer open past the edge of the window. A menu that does not fit above its target flips below it, so corrections for a word near the top of a document are no longer pushed out of view.
- The OS keychain is consulted once per provider per session rather than on every rewrite, which on macOS raised a password prompt each time. A local endpoint never consults it.
- The endpoint reachability check is gated on the same per-host consent as a rewrite, and reports a missing consent as its result rather than as a connection failure. Picking a hosted preset previously sent a request with the API key attached before any dialog appeared. The consent text names the key and the check.
- A `.deb` install can update itself. The update feed serves the signed `.deb`; every Linux install was previously offered the AppImage, so the update failed after downloading it.
- The macOS `.pkg` is signed with a Developer ID Installer certificate, notarized, and stapled. A release that has the installer certificate set but produces an unsigned pkg now fails instead of publishing it.
- On the AUR package, `/usr/bin/writ` is the CLI, so piping and `--title` work. It previously pointed at the app, which reads argv as paths and ignores stdin. The desktop entry still launches the app.
- `writ <file>` opens the file in Writ on Linux and Windows. It previously went through the desktop default handler, which could open another editor, since Writ registers its file associations at Alternate rank.
- An empty piped payload opens nothing. It previously wrote a zero-byte file and opened a blank buffer.
- `writ --version` reports the installed version. It printed 0.1.0 on every release since the CLI landed.

## [0.2.0] - 2026-07-24

### Added

- Local spell check for text buffers: likely misspellings get a wavy underline while code, URLs, and links stay clear, with a status-bar item carrying the on/off switch, fix-all, and a per-word preview. Off by default and fully offline.
- Opt-in rewrite of selected text: proofread, rephrase, polish, or a custom instruction, streamed into a side-by-side original/result view before applying. Runs on a local model by default; hosted providers use a key stored in the OS keychain, and text is sent only when a rewrite is run.
- Line operations with editor-scoped shortcuts: duplicate, delete, move, and join lines, select line, insert a line above or below, select next occurrence, and add a cursor above or below.

### Changed

- Text transforms renamed to plain labels with descriptions of what each one does.
- The landing site's interactive window now runs the app's actual editor engine, sharing the editor modules, command tables, and theme tokens with the app. The site adds shortcut, terminal, and theme sections generated from the same sources, and the download section lists the Homebrew, winget, and curl installs.

### Fixed

- The AUR package installs the desktop entry and icons.
- The CLI install status no longer reports a dangling `writ` link as installed.

## [0.1.0] - 2026-07-05

### Added

- Cargo workspace of four library crates (`writ-core`, `writ-storage`, `writ-plugin`, `writ-render`), the `writ-cli` binary, and the `src-tauri` Tauri shell.
- `writ-core`: buffer model with create, update, delete, and reorder operations; workspace and tab ordering; command registry for palette actions; history module for undo/redo and closed-tab recovery; file-watcher integration; full-text search query policy; typed error hierarchy.
- `writ-storage`: SQLite in WAL mode with connection pooling, schema migrations, and FTS5 full-text search; `BufferStore` and `ConfigStore` repositories with session snapshots, dirty-shutdown detection, and a startup consistency check. A dirty relaunch recovers buffers from the latest snapshot and the consistency pass logs orphaned or missing backing files.
- Global hotkey (`Cmd+Shift+Space` / `Ctrl+Shift+Space`) toggles the main window; window position persists across quit.
- Tabbed editor on CodeMirror 6 with language detection by extension and file content, live Markdown typography for `.md` buffers, and reopen-closed recovery.
- Split-pane live preview over a `writ-preview://` protocol: Markdown, HTML, Mermaid diagrams, and KaTeX math rendered from bundled offline runtimes, with source and preview scroll sync and find inside the preview.
- Find and replace overlay with a live match count.
- Full-text search across buffers, with a results panel showing snippets and line numbers.
- Command palette on double-tap `Shift`.
- Settings window (`Cmd+,`) with searchable settings indexed into the palette, plus configurable keyboard shortcuts.
- Editor and preview font zoom.
- Status bar with language, encoding, and cursor position.
- Prompt fill modal with placeholder variables, a live token estimate, and copy as prompt.
- Text transforms: Tidy Whitespace and composable trailing-trim, final-newline, punctuation-spacing, and dedent passes.
- Workspace folders: open a folder with a sidebar file tree.
- Watched-folder inbox: new files in a watched folder list in the sidebar and open as they arrive.
- File-size policy with a large-file mode and a binary hex view.
- `writ` command-line tool, installed as a sidecar, for opening files from the terminal.
- Make Writ the default app on macOS for Markdown, plain text and logs, config and data files, and source files.
- App-wide light and dark themes routed through design tokens at WCAG AA contrast.
- Autosave on every keystroke; buffers persist across restarts; crash recovery restores the last session.
- TOML configuration with hot reload.
- Self-update via `tauri-plugin-updater` with signed manifests.
- Landing site built with Astro on a shared design system.
- Release pipeline: GitHub Actions matrix build for macOS universal, Windows x64, and Linux x64; signed bundles; SHA256 checksums; updater manifest; Homebrew, winget, and AUR distribution manifests with auto-bump on release.

### Documentation

- README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, RELEASING.
- Architecture overview and a numbered ADR series (`docs/adr/`) covering the foundational decisions (Tauri over Electron, SolidJS over React, CodeMirror over Monaco, SQLite over flat files, and the Cargo workspace split) plus every subsequent design decision.
