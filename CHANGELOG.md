# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
