# Features

What Writ does, at the level of detail that answers "does it do X".

## Files

- Writ opens any text file from Finder, the Dock, the Open dialog (`Cmd+O`) or the `writ` command, and saves it back to its own path.
- New files go in one folder, `~/Writ` unless you pick another under Settings, Files, Folder. The file on disk is the only copy of its text.
- A new file (`Cmd+N`) is `.txt` or `.md`, set under Settings, Files, Default format. A file nobody named is called `writ-<yymmdd>-<hhmm>`, and a second one in the same minute gets `-2`.
- File > Today's File opens a file named for today's date, in the same format.
- Renaming a file to a name ending in `.md`, `.markdown`, `.txt` or `.text` changes its format. Any other name keeps the file's own extension.
- Tabs, the file list and the palette show a name without its `.md` or `.txt`. Other extensions stay visible.
- Autosave writes one second after the last keystroke, and at once when the window loses focus or hides. A save that would overwrite a newer version on disk is refused, and your text is written beside the file as a dated conflict copy.
- When a file changes outside Writ, a tab with no unsaved edits takes the new text in one undoable step. A tab with unsaved edits asks which side to keep.
- Writ keeps a version each time a file is saved, for up to 30 days or 200 versions, outside the folder. File > Revert To puts one back.
- Open tabs and unsaved text come back after a restart or a crash.
- Binary files open as hex. Large files open with syntax colouring off.
- On macOS, Writ can register as the app that opens text, config, data and source files (Settings, Files).

## Markdown

- A Markdown file opens in Inline mode: headings at heading sizes, bold, italic, strikethrough and inline code styled, links with the address hidden, images shown under their line, task lists as checkboxes, and fenced code highlighted in its language. The line you are editing shows its markup.
- The Inline | Source switch in the status bar shows the raw Markdown instead. Settings, Preview sets which one a Markdown file opens in.
- `[[Name]]` links another file by name, completes from the files in the folder, and opens on `Cmd+click` or `Enter`. `[[Name#Heading]]` opens at the heading. A name two files share asks which one is meant.
- `Cmd+B`, `Cmd+I`, `Cmd+Shift+X`, `Cmd+Shift+E` and `Cmd+K` format a selection. Settings, Editor, Markdown shortcuts turns them off.
- A `.txt` file gets the plain editor, with no link completion.

## Search

- `Cmd+Shift+F` searches commands, settings, file names and the text of every file in the folder in one list.
- Prefixes narrow it: `>` commands, `#` text, `:` go to a line, `@` file names.
- `Cmd+Shift+O` opens a file by name.
- `Cmd+F` finds in the current file, `Cmd+Option+F` replaces.

## Editing

- CodeMirror 6 with language detection.
- The command palette opens on a double tap of `Shift`.
- Line commands: duplicate (`Cmd+D`), delete (`Cmd+Shift+K`), move (`Shift+Alt+Up/Down`), select (`Cmd+L`), join (`Cmd+Shift+J`), insert below or above (`Cmd+Enter`, `Cmd+Shift+Enter`), toggle comment (`Cmd+/`) and select next occurrence (`Cmd+Shift+D`).
- Text transforms in the palette: trim leading or trailing spaces, collapse repeated spaces, straighten quotes, remove shared indentation, end with one newline, fix spacing before punctuation, prepare as prompt, and tidy whitespace, which runs several of them.
- Fill placeholders asks for a value for each `{{placeholder}}` and copies the result, with a token estimate. Copy as Prompt strips frontmatter and comments and copies the rest.
- Right-click opens Writ's own menu: spelling corrections, link actions, clipboard, rewrite actions on a selection when Rewrite is on, and a search seeded with the selection.
- The status bar shows the cursor position, the encoding and the layout switch. Word, character and token counts are behind a switch in Settings, Editor.

## Preview

- HTML and Mermaid (`.mmd`) files open with a preview pane: side by side, preview only or source only. `Cmd+Shift+H` swaps the split orientation.
- The preview blocks network access. See [adr/011-preview-trust-model.md](adr/011-preview-trust-model.md).
- A link clicked in the preview names its destination host and asks before opening.

## Links in any file

- `Cmd+click` (`Ctrl+click` on Windows and Linux) opens `http`, `https` and `mailto` links. Links are underlined only while the modifier is held.
- A relative link such as `[spec](./docs/spec.md)` opens in Writ when it resolves inside the folder.

## Window

- `Cmd+Shift+Space` (`Ctrl+Shift+Space` on Windows and Linux) shows and hides the window from any app. It and every other shortcut can be changed under Settings, Shortcuts.
- Tabs switch with `Cmd+[` and `Cmd+]`, and `Cmd+Shift+T` reopens the last closed one.
- Light and dark follow the system, with six accent colours, preset themes and custom colours.
- The editor font size (`Cmd+=`, `Cmd+-`, `Cmd+0`) and the interface text size (Settings, Appearance) change separately.

## Apps

Six features are apps, each switched on under Settings, Apps, and all off on a fresh install. An app that is off has no menu item, palette command, shortcut or panel.

- **Chat**: a pane beside the file (`Cmd+Shift+A`) with the open file attached; `@` attaches another. Only attached files are sent, and the send dialog names the host and the byte count first. A proposed edit shows as a line diff with Apply and Discard. Conversations are kept and can be renamed or deleted.
- **Rewrite**: proofread, rephrase, polish, improve prompt (which keeps `{{placeholders}}` as written), or your own instruction, on a selection.
- **Connected programs**: an MCP server, `writ mcp`, that another program starts over stdio. Eight read tools and three write tools (write, create, rename), with separate read and write switches per program and nothing that deletes. Every program is refused until you approve it, and an Activity panel lists every call.
- **Connections**: a panel (`Cmd+Shift+\`) listing the files that link to the open one with the sentence each link sits in, its outline of headings, and its frontmatter properties.
- **Graph**: the files around the open one, and a view of the whole folder with search and one colour per top-level folder.
- **Tags**: `#tags` and frontmatter tags in the sidebar with counts. Nested tags group under their parent, and selecting one filters the file list.

Chat and Rewrite share one AI connection: Ollama or LM Studio on this machine, Anthropic, OpenAI, Google Gemini, OpenRouter, Groq, DeepSeek, Mistral, xAI, Together or Fireworks with a key, or any OpenAI-compatible endpoint. The key goes in the system keychain on macOS and Windows, and in memory for the session elsewhere. Writ asks before the first send to a host off your machine.

## More

- Spell check runs on this machine and is off until you turn it on. Code, URLs and identifiers stay unflagged.
- A watched folder (Settings, Advanced) opens new files as they arrive. See [adr/018-watch-inbox.md](adr/018-watch-inbox.md).
- The `writ` command opens files, folders and piped text from a terminal, and answers questions about the folder. See [cli-verbs.md](cli-verbs.md).
- A folder written in Obsidian opens as it is: [what carries over](./importing-from-obsidian.md).
- No account and no telemetry.
- Updates verify a signed manifest, and the automatic check can be turned off under Settings, Updates. See [adr/007-in-app-updater.md](adr/007-in-app-updater.md).
