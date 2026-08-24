# Features in full

The [README](../README.md) lists the eight that describe the product. This is the rest, at the level of detail that answers "does it do X".

## Window and session

- Global hotkey summons the window from anywhere: `Cmd+Shift+Space` on macOS, `Ctrl+Shift+Space` on Windows and Linux. The app starts hidden and stays resident, so the hotkey shows a window rather than booting a program.
- Autosave on every keystroke. Buffers persist across restarts and crash recovery restores the last session.
- Browser-style tabs with reopen-closed (`Cmd+Shift+T`), light and dark themes, and separate font zoom for the editor and the preview.
- State lives in `~/.writ`: buffer text as plain files under `buffers/`, metadata and the search index in `writ.db`, anything piped into the CLI in `piped/`, plus `config.toml` and `logs/`.

## Search

- `Cmd+Shift+F` searches commands, settings, and every buffer, open or from history, in one palette. With a workspace folder open it adds file names and greps file contents on each query.
- Prefixes: `>` for commands, `#` for content, `:` to go to a line.
- `Cmd+F` finds within the current document.

## Editing

- CodeMirror 6 with language auto-detection, live Markdown typography, and formatting shortcuts.
- Command palette on double-tap `Shift`.
- Text transforms built from small composable passes: trim leading or trailing spaces, collapse repeated spaces, straighten quotes, remove shared indentation, end with one newline, fix spacing before punctuation, prepare as prompt, and Tidy Whitespace which runs several of them.
- Right-click opens Writ's own menu rather than the webview's, everywhere but the preview pane: spelling corrections, link actions, clipboard, rewrite actions on a selection, and a workspace search seeded with the selection.

## Preview

- Split-pane live preview with scroll sync: Markdown, HTML, Mermaid diagrams and KaTeX math, all rendered from runtimes bundled into the app.
- The preview blocks network access. See [adr/011-preview-trust-model.md](adr/011-preview-trust-model.md).
- Binary files open as hex, and large files open with syntax highlighting off rather than refusing to open.

## Links

- `Cmd+click` (`Ctrl` elsewhere) opens `http`, `https` and `mailto` links from the editor. Links are underlined only while the modifier is held.
- A relative link such as `[spec](./notes/spec.md)` opens in Writ when it resolves inside the workspace.
- A link clicked in the preview names its destination host and asks before opening.

## Spell check

- Runs locally, so nothing leaves the machine. Off by default.
- Double-click a flagged word for corrections in place: accept one, fix all, or add the word to your dictionary.
- Code, URLs, and tokens like `API` or `useSignal` stay unflagged.

## Rewrite and prompts

- Rewrite a selection: proofread, rephrase, polish, improve prompt (instructed to leave `{{placeholders}}` intact), or your own instruction.
- A local model (Ollama) by default, or any OpenAI-compatible endpoint with your own key. The key is kept in the OS keychain on macOS and Windows, and in memory for the session elsewhere.
- Off until you turn it on. Writ asks before the first send to a host off your machine, and only the text you rewrite is sent.
- Prompt fill: placeholder variables, a live token estimate, and copy as prompt.

## Files coming from elsewhere

- Workspace folders with a file tree.
- A watched inbox that opens new files as they arrive. See [adr/018-watch-inbox.md](adr/018-watch-inbox.md).
- A `writ` CLI for opening files and piped input from the terminal. See [adr/017-command-line-surface.md](adr/017-command-line-surface.md).
- Registration as the default app for text, config and source files on macOS.

## Updates and privacy

- Local-only storage, no account, no telemetry.
- Self-updates verify a signed manifest and can be turned off. See [adr/007-in-app-updater.md](adr/007-in-app-updater.md).
