# Writ

A lightweight, always-ready text editor for developers.

[![CI](https://img.shields.io/github/actions/workflow/status/ibrahemid/writ/ci.yml?branch=main&label=CI&logo=github)](https://github.com/ibrahemid/writ/actions)
[![Release](https://img.shields.io/github/v/release/ibrahemid/writ?include_prereleases&sort=semver)](https://github.com/ibrahemid/writ/releases/latest)
[![License](https://img.shields.io/github/license/ibrahemid/writ)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)](https://github.com/ibrahemid/writ/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/ibrahemid/writ/total)](https://github.com/ibrahemid/writ/releases)

![Writ demo](docs/assets/hero.gif)

## Download

[**Download for macOS**](https://github.com/ibrahemid/writ/releases/latest) · [**Download for Windows**](https://github.com/ibrahemid/writ/releases/latest) · [**Download for Linux**](https://github.com/ibrahemid/writ/releases/latest)

All builds come from the same source tree. Pick the installer for your OS on the [latest release page](https://github.com/ibrahemid/writ/releases/latest).

## Features

- Global hotkey summons the window from anywhere, `Cmd+Shift+Space` on macOS, `Ctrl+Shift+Space` on Linux and Windows
- Autosave on every keystroke; buffers persist across restarts
- Full-text search across every buffer, backed by SQLite FTS5
- CodeMirror 6 editor with language auto-detection for 50+ languages
- Browser-style tabs with reorder and reopen-closed support
- Local-only storage, no network, no telemetry, no account

## Why Writ?

A plain text file has no tabs, no search across buffers, and no persistent autosave. Obsidian is a knowledge graph for permanent notes, overkill for a five-second scratch. Notion is a cloud workspace, the opposite of instant and offline. Writ is the missing middle: a tray-resident scratchpad that opens on a keypress, saves as you type, and stays out of your way. It is optimized for the throwaway buffer, the paste target between terminals, the half-formed idea you need to capture in two seconds.

## See it in action

Visit the [landing page](https://ibrahemid.github.io/writ) for the full demo and feature tour. Static screenshots live in [docs/assets/](docs/assets/).

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Toggle window | `Cmd+Shift+Space` |
| New tab | `Cmd+T` |
| Close tab | `Cmd+W` |
| Switch tabs | `Cmd+[` / `Cmd+]` |
| Reopen closed tab | `Cmd+Shift+T` |
| Command palette | `Shift+Shift` |
| Toggle sidebar | `Cmd+SS` (double tap) |
| Rename tab | Double-click tab |
| Search buffers | `Cmd+F` (sidebar) |

Buffers are stored in a local SQLite database under your OS's standard application data directory.

## Build from source

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

## Tech stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri v2 |
| Frontend | SolidJS + Vite |
| Editor | CodeMirror 6 |
| Storage | SQLite (WAL mode, FTS5) |
| Core logic | Rust: `writ-core`, `writ-storage`, `writ-plugin` |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full system design and [docs/adr/](docs/adr/) for architecture decision records.

## Contributing

Contributions welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow, coding conventions, and pull request process. Security issues go through [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).

## Star history

[![Star History](https://api.star-history.com/svg?repos=ibrahemid/writ&type=Date)](https://star-history.com/#ibrahemid/writ&Date)

Made with Tauri and SolidJS.
