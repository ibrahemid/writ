# Writ

**Writ — A lightweight, always-ready text editor for developers**

A distraction-free scratchpad that lives in your system tray. Summon it with a global hotkey, write something, and it saves itself. No dialogs, no friction.

<!-- screenshot -->

## Features

- **Global hotkey** — toggle the window from anywhere with `Cmd+Shift+Space` (macOS) or `Ctrl+Shift+Space` (Linux/Windows)
- **Browser-like tabs** — open multiple buffers, reorder them, restore closed ones
- **Autosave** — every buffer saves automatically; nothing is lost on crash
- **Full-text search** — search across all buffers instantly with FTS5
- **CodeMirror 6** — syntax highlighting for 50+ languages, detected automatically by extension or shebang
- **Dark theme** — easy on the eyes, built with CSS custom properties
- **Command palette** — `Cmd+Shift+P` for all actions without leaving the keyboard
- **Session recovery** — crash detection restores your last session on relaunch

## Installation

### Build from Source

**Prerequisites**

- Rust 1.77+ (`rustup`)
- Node.js 20+
- pnpm 9+ (`npm install -g pnpm`)
- Platform Tauri prerequisites: [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)

**Steps**

```bash
git clone https://github.com/your-org/writ.git
cd writ
pnpm install
cargo tauri dev
```

To produce a release build:

```bash
cargo tauri build
```

The installer or app bundle will be in `src-tauri/target/release/bundle/`.

## Usage

| Action | Shortcut |
|---|---|
| Toggle window | `Cmd+Shift+Space` |
| New tab | `Cmd+N` |
| Close tab | `Cmd+W` |
| Reopen closed tab | `Cmd+Shift+T` |
| Command palette | `Cmd+Shift+P` |
| Toggle sidebar | `Cmd+B` |
| Search buffers | `Cmd+F` (sidebar) |

The sidebar shows active tabs and browsable history. Buffers are stored locally in an SQLite database; nothing leaves your machine.

## Tech Stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri v2 (Rust) |
| Frontend | SolidJS + Vite |
| Editor | CodeMirror 6 |
| Storage | SQLite (WAL mode, FTS5) |
| Config | TOML with hot-reload |
| Core logic | Rust — `writ-core`, `writ-storage`, `writ-plugin` crates |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Architecture decisions are documented in [docs/adr/](docs/adr/).

## License

[MIT](LICENSE) — Copyright 2026 Writ Contributors.
