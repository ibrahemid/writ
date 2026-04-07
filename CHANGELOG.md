# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

#### Phase 1 — Core Foundation

- Cargo workspace with four crates: `writ-core`, `writ-storage`, `writ-plugin`, `writ-cli`
- `writ-core`: buffer model (`document.rs`, `manager.rs`) with create, update, delete, and reorder operations
- `writ-core`: workspace management and tab ordering
- `writ-core`: command registry (`command/registry.rs`) for palette-driven actions
- `writ-core`: history module for undo/redo and closed-tab recovery
- `writ-core`: file watcher integration for external change detection
- `writ-core`: typed error hierarchy (`errors.rs`) with no use of generic `Error`
- `writ-storage`: SQLite database layer with WAL mode and connection pooling (`database/connection.rs`)
- `writ-storage`: schema migrations runner (`database/migrations.rs`)
- `writ-storage`: FTS5 full-text search index with trigram tokenizer (`fts.rs`)
- `writ-storage`: `BufferStore` and `ConfigStore` repository implementations
- `writ-storage`: crash recovery module (`recovery/`) detecting unclean shutdowns and restoring session
- `writ-storage`: consistency checker (`consistency.rs`) to repair orphaned or corrupted rows
- TOML configuration loading with hot-reload support (`config/`)

#### Phase 2 — Tauri IPC & App Shell

- Tauri v2 application shell (`src-tauri/`) with system tray icon
- Global hotkey registration (`hotkey/`) toggling the main window with `Cmd+Shift+Space` / `Ctrl+Shift+Space`
- IPC command surface: buffer CRUD (`commands/buffer.rs`), config reads/writes (`commands/config.rs`), history access (`commands/history.rs`), window management (`commands/window.rs`)
- File watcher integration in Tauri (`watcher/`) emitting events to the frontend
- Structured application logging (`logging/`) with contextual fields (operation, buffer ID)
- App state container (`state.rs`) wiring core, storage, and Tauri managed state
- Tauri event bridge (`events/`) for push notifications from backend to frontend

#### Phase 3 — SolidJS Frontend

- SolidJS + Vite frontend with strict TypeScript configuration
- `Editor` component wrapping CodeMirror 6 with language auto-detection by file extension and shebang
- `TitleBar` component with draggable tab strip, new-tab button, and close-tab button
- `Sidebar` component showing active tabs, buffer history, and full-text search input
- `CommandPalette` component (`Cmd+Shift+P`) listing all registered commands with fuzzy filtering
- `ErrorBoundary` component for graceful rendering failure recovery
- `Notifications` component for transient toast messages
- `buffers` store managing tab state, active buffer, and reorder operations
- `editor` store tracking cursor position, scroll state, and dirty flag
- `sidebar` store for sidebar open/closed state and search query
- `config` store consuming Tauri config events with hot-reload
- Frontend service layer (`services/`) wrapping Tauri IPC calls with typed return values
- Dark theme with CSS custom properties and no hard-coded colour values
- Keyboard shortcut handling: `Cmd+N` new tab, `Cmd+W` close tab, `Cmd+Shift+T` reopen closed tab, `Cmd+B` toggle sidebar
- Autosave debounce wired through the editor store — no manual save required
