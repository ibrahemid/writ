# Writ

A lightweight, always-ready text editor for developers.

Writ is a distraction-free scratchpad that lives in your system tray. It launches instantly, saves continuously, and disappears when you are done. There are no dialogs, no project setup, no "save as", and nothing leaves your machine.

## Why

Every developer keeps a scratch file somewhere: a notes buffer, a throwaway snippet, a paste target between two terminals. Editors designed for projects are the wrong shape for this. Writ is built for the five-second write, with a global hotkey, autosave, and a minimal binary footprint.

## Features

- Global hotkey to toggle the window from anywhere (`Cmd+Shift+Space` on macOS, `Ctrl+Shift+Space` on Linux and Windows)
- Browser-like tabs with reorder and reopen-closed support
- Autosave on every edit, with crash recovery on relaunch
- Full-text search across buffers backed by SQLite FTS5
- CodeMirror 6 editor with language auto-detection for 50+ languages
- Command palette with fuzzy matching (`Cmd+Shift+P`)
- Dark theme driven entirely by CSS custom properties
- Local-only storage; no network, no telemetry

## Install

### From source

Prerequisites: Rust 1.77+, Node.js 20+, pnpm 9+, and the [Tauri v2 platform prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
git clone https://github.com/ibrahemid/writ.git
cd writ
pnpm install
cargo tauri dev
```

For a release build:

```bash
cargo tauri build
```

The installer or app bundle is written to `src-tauri/target/release/bundle/`.

## Shortcuts

| Action | Shortcut |
|---|---|
| Toggle window | `Cmd+Shift+Space` |
| New tab | `Cmd+N` |
| Close tab | `Cmd+W` |
| Reopen closed tab | `Cmd+Shift+T` |
| Command palette | `Cmd+Shift+P` |
| Toggle sidebar | `Cmd+B` |
| Search buffers | `Cmd+F` (sidebar) |

Buffers are stored in a local SQLite database under your OS's standard application data directory.

## Architecture

Writ is a Cargo workspace with compiler-enforced crate boundaries and a thin Tauri adapter over pure Rust core logic.

| Layer | Technology |
|---|---|
| Desktop shell | Tauri v2 |
| Frontend | SolidJS + Vite |
| Editor | CodeMirror 6 |
| Storage | SQLite (WAL mode, FTS5) |
| Core logic | Rust: `writ-core`, `writ-storage`, `writ-plugin` |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full system design and [docs/adr/](docs/adr/) for architecture decision records.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow, coding conventions, and pull request process. Security issues go through [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
