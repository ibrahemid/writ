# ADR-055: Quick capture is a second window

## Status

Accepted, 2026-10-15.

## Context

Adding one line to Today's File took the summon chord, the full window with its tabs, the File menu, typing and hiding the window again. AGENTS.md allows module-level state only as single-window singletons, and every store under `src/stores/` is one.

## Decision

1. **A second webview window, `capture`**, not a compact mode of the main window. A compact mode would resize the person's window and move its saved place, carry the tabs, and have to put the window back on Escape rather than hide it.
2. **Its own entry and realm.** `capture.html` loads `src/capture/main.tsx`, which renders `CaptureNote` over `stores/capture/capture-note.ts` and `services/tauri.ts`. A webview is its own JavaScript realm, so the main bundle's singletons (every module in `stores/global/` and `stores/window/`, `commands/app-commands.ts` and the components marked `// Singleton state`) never load there. `architecture/capture-entry.test.ts` fails on an import that reaches them. The capture page never sends `frontend-ready`, listens for no event, holds no capability, and takes the theme from the token set the main window saves for the same origin.
3. **Rust's single-window points stay the main window's**: `get_webview_window("main")` in the toggle, the reveal, the Dock reopen and file opens; `frontend_ready` and the quit flush; `capabilities/default.json` (`windows: ["main"]`). The capture window has drag and drop off, so `DragDrop` only comes from the main window.
4. **Built hidden once the main window's frontend is ready** while the capture chord is held, or on the first press otherwise: always on top, on every Space, off the taskbar, without decorations. A close request hides it.
5. **The capture chord** (`hotkey.capture`, `CmdOrCtrl+Shift+Alt+Space`) is registered beside the summon chord with its own held state, so either can be taken, turned down or rebound without touching the other. One chord cannot hold both. An empty value turns capture off.
6. **The append** finds Today's File the way the File menu does, takes the note's turn when a tab holds it, reads the file once, adds the entry at the end in the file's encoding and line ending, and writes through the guard against what it read, keeping a version. A file that changes between that read and the rename is read again, three times at most. A missing file is minted holding the entry, with no tab. A tab holding the file is told as the watcher would tell it, and one with unsaved text gets the change bar.
7. **Focus.** The app in front when the chord was pressed is remembered (NSWorkspace on macOS, the foreground window on Windows) and brought back once the entry is written; when it was Writ, the main window is focused. A failed append keeps the window, the text and the reason. Clicking away hides the note and keeps its text for the next press.

## Consequences

- The resident app holds a second web view.
- On Linux the window manager decides where focus goes when the note hides.
- Capture works only where a global chord does: a Wayland session reports both chords as unsupported.
