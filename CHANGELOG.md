# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Cargo workspace split into three library crates plus the Tauri shell: `writ-core`, `writ-storage`, `writ-plugin`, `src-tauri`.
- `writ-core`: buffer model with create, update, delete, and reorder operations; workspace and tab ordering; command registry for palette-driven actions; history module for undo/redo and closed-tab recovery; file watcher integration; typed error hierarchy.
- `writ-storage`: SQLite backend in WAL mode with connection pooling, schema migrations, and FTS5 full-text search.
- `writ-storage`: `BufferStore` and `ConfigStore` repositories. Snapshot, dirty-shutdown, and consistency-check infrastructure is scaffolded in the crate but is not yet wired into the running app.
- TOML configuration loading with hot reload.
- Tauri v2 shell with a global hotkey (`Cmd+Shift+Space` / `Ctrl+Shift+Space`) toggling the main window, structured logging with contextual fields, and an event bridge from backend to frontend.
- IPC command surface for buffer CRUD, file open, config reads and writes, history access, and window management.
- SolidJS + Vite frontend in strict TypeScript: editor wrapping CodeMirror 6 with language auto-detection by extension and shebang; tab strip with reopen-closed recovery; sidebar listing tabs, history, and full-text search; command palette (double-tap `Shift`); error boundary; toast notifications.
- Stores for buffers, editor state, sidebar, and config; service layer wrapping Tauri IPC with typed returns.
- Keyboard shortcuts: `Cmd+T` new tab, `Cmd+W` close, `Cmd+[` / `Cmd+]` switch, `Cmd+Shift+T` reopen closed, `Cmd+S` toggle sidebar.
- Autosave debounce wired through the editor store; no manual save.
- Self-update via `tauri-plugin-updater` with signed manifests.
- Astro landing page with four themes, real product screenshots, and zero JS chunks.
- Release pipeline: GitHub Actions matrix build for macOS universal, Windows x64, and Linux x64; signed bundles; SHA256 checksums; updater manifest; Homebrew, winget, and AUR distribution manifests with auto-bump on release.

### Documentation

- README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, RELEASING.
- Architecture overview and a numbered ADR series (`docs/adr/`) covering the foundational decisions — Tauri over Electron, SolidJS over React, CodeMirror over Monaco, SQLite over flat files, and the Cargo workspace split — plus every subsequent design decision.
