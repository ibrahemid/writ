# Features

What Writ does, at the level of detail that answers "does it do X".

## Files

- Writ opens any text file from Finder, the Dock, the Open dialog (`Cmd+O`) or the `writ` command, and saves it back to its own path.
- New files go in one folder, `~/Writ` until you move it with Move… under Settings, General, Folder. The file on disk is the copy Writ reads and saves; its data folder keeps a search index and earlier versions of it.
- A new file (`Cmd+N`) is `.txt` or `.md`, set under Settings, General, Default format. A file nobody named is called `writ-<yymmdd>-<hhmm>`, and a second one in the same minute gets `-2`.
- File > Today's File opens a file named for today's date, in the same format.
- Renaming a file to a name ending in `.md`, `.markdown`, `.txt` or `.text` changes its format. Any other name keeps the file's own extension.
- Tabs, the file list and the palette show a name without its `.md` or `.txt`. Other extensions stay visible.
- Autosave writes one second after the last keystroke, and at once when the window loses focus or hides. A save that would overwrite a newer version on disk is refused, and your text is written beside the file as a dated conflict copy.
- When a file changes outside Writ, a tab with no unsaved edits takes the new text in one undoable step. A tab with unsaved edits asks which side to keep.
- Writ keeps a version of a file in its folder when it is saved, for up to 30 days or 200 versions, outside that folder; a run of saves inside ten seconds is one version. A file over 2 MB is not versioned. File > Revert To puts one back and names who wrote each version: Writ, a connected program, chat or the writ command.
- Open tabs and unsaved text come back after a restart or a crash.
- Binary files open as hex. Large files open with syntax colouring off.
- A file over 50 MiB asks before it opens and says how large it is. A file over 500 MiB (524 MB) doesn't open, and Writ says why. Writ measures the tabs it restores at launch as it starts: one over 50 MiB asks when it is shown, and one over 500 MiB says why.
- On macOS, Writ can register as the app that opens text, config, data and source files (Settings, Integrations).

## Markdown

- A Markdown file opens in Inline mode: headings at heading sizes, bold, italic, strikethrough and inline code styled, links with the address hidden, images shown under their line, task lists as checkboxes, fenced code highlighted in its language, and tables, `$…$` and `$$…$$` math and `mermaid` diagrams drawn in place. The line you are editing shows its markup, and a table, formula or diagram shows its source while the cursor is in it.
- A heading folds its section from the arrow beside it, and `Cmd+Option+[` and `Cmd+Option+]` (`Ctrl+Shift+[` and `Ctrl+Shift+]` on Windows and Linux) fold and unfold at the cursor. A source file folds from the gutter beside its line numbers.
- The Inline | Source switch in the status bar shows the raw Markdown instead. Settings, Editor, Markdown opens in sets which one a Markdown file opens in.
- `[[Name]]` links another file by name, completes from the files in the folder, and opens on `Cmd+click` or `Enter`. `[[Name#Heading]]` opens at the heading. A name two files share asks which one is meant.
- `Cmd+B`, `Cmd+I`, `Cmd+Shift+X`, `Cmd+Shift+E` and `Cmd+K` format a selection. Settings, Editor, Markdown shortcuts turns them off.
- A `.txt` file gets the plain editor, with no link completion.

## Search

- `Cmd+Shift+F` (Edit > Search Everywhere) searches commands, settings, file names and the text of the files in the folder, up to 5 MB each, in one list.
- Prefixes narrow it and the command palette alike: `>` commands, `@` file names (in Search Everywhere, of open, recent and Open Folder files), `#` text, `:` go to a line. Both palettes list them along the bottom, and a click on one narrows what is typed.
- `Cmd+Shift+O` opens a file by name.
- The sidebar's search field searches the text of Writ's folder and of a folder opened with Open Folder, with each folder's matches under its name. Typing opens the sidebar on the results; the arrow keys walk them, `Enter` opens one at its line, and `Escape` clears the field and returns to the file.
- `Cmd+F` finds in the current file, `Cmd+Option+F` replaces.

## Editing

- CodeMirror 6. A file's name decides its language, and a `.txt` or `.text` file stays plain text whatever it holds. Change Language in the palette picks another language for one file, and the file keeps it.
- Word wrap, tab size and reading width, under Settings, Editor, apply to open files at once. Reading width keeps lines of prose to a set width; code files keep the full width.
- The command palette opens on a double tap of `Shift` (Help > Command Palette). Typing a file's name there lists the file under the commands.
- File > Open Folder (`Cmd+Option+O`) shows a folder's files in the sidebar. The tree draws only the rows on screen; it is one `Tab` stop, with the arrow keys, `Home`, `End` and typing a name to move inside it. A folder Writ cannot read says so with Try again, and folders stay open across launches. Right-click on any row shows it in the file manager or copies its path; in Writ's folder it also renames, moves to the Trash, or makes a new file in a folder.
- Help holds Writ Help (the guide on the site), Keyboard Shortcuts, What's New (the changelog at this version), Show Setup Again and Report an Issue (the bug form, with the version filled in).
- Line commands: duplicate (`Cmd+D`), delete (`Cmd+Shift+K`), move (`Shift+Alt+Up/Down`), select (`Cmd+L`), join (`Cmd+Shift+J`), insert below or above (`Cmd+Enter`, `Cmd+Shift+Enter`), toggle comment (`Cmd+/`) and select next occurrence (`Cmd+Shift+D`).
- Text transforms in Edit > Transform, in the right-click menu on a selection, and in the palette: trim leading or trailing spaces, collapse repeated spaces, straighten quotes, remove shared indentation, end with one newline, fix spacing before punctuation, prepare as prompt, and tidy whitespace, which runs several of them. A transform acts on the selection, or on the whole file when nothing is selected; text typed elsewhere in the file while it runs stays, and an edit inside the text it acts on leaves the text as it is.
- Fill placeholders asks for a value for each `{{placeholder}}`, shows the filled text, and copies it, with a token estimate. Copy as prompt (Edit menu, right-click menu, palette) strips frontmatter and comments and copies the rest.
- Copy files as prompt (palette) puts several files on the clipboard as one prompt: the file in front with the files it links to, the files of its folder, or the files carrying a tag. It shows the files, a token estimate for each and for the whole, the files it could not read, and the text before copying, and holds up to 100 files and 4 MB.
- Right-click opens Writ's own menu: spelling corrections, link actions, clipboard with Copy as prompt, the Rewrite actions on a selection when Rewrite is on, the Transform actions on a selection, and a search seeded with the selection. Copy and Cut act on every selection range.
- A file in another text encoding (Windows-1252, Shift JIS, GBK, UTF-16 and others) opens as text and saves in that encoding. A save that would lose a character the encoding cannot hold is refused, and the character is named.
- The status bar shows the cursor position, the file's encoding and the layout switch. The encoding reopens the file in another one. Word, character and token counts are behind a switch in Settings, Editor.

## Preview

- HTML and Mermaid (`.mmd`) files open with a preview pane: side by side, preview only or source only. `Cmd+Shift+H` swaps the split orientation.
- The preview blocks network access. See [adr/011-preview-trust-model.md](adr/011-preview-trust-model.md).
- A link clicked in the preview names its destination host and asks before opening.

## Links in any file

- `Cmd+click` (`Ctrl+click` on Windows and Linux) opens `http`, `https` and `mailto` links. Links are underlined only while the modifier is held.
- A relative link such as `[spec](./docs/spec.md)` opens in Writ when it resolves inside the folder.

## Window

- `Cmd+Shift+Space` (`Ctrl+Shift+Space` on Windows and Linux) shows and hides the window from any app. Settings, General records a new one, and Settings, Shortcuts lists it first with every other shortcut.
- `Cmd+Shift+Option+Space` (`Ctrl+Shift+Alt+Space` on Windows and Linux) opens a small window over any app. Escape adds what you typed to the end of Today's File; on macOS and Windows the app you were in gets the keyboard back.
- Tabs switch with `Cmd+[` and `Cmd+]`, and `Cmd+Shift+T` reopens the last closed one.
- Light and dark follow the system, with six accent colours, preset themes and custom colours.
- The editor font size (`Cmd+=`, `Cmd+-`, `Cmd+0`) and the interface text size (Settings, Appearance) change separately.

## Apps

Six features are apps, each switched on under Settings, Apps, and all off on a fresh install. An app that is off has no menu item, palette command or panel; its shortcut opens its row in Settings, Apps. Each row and setup card lists where the app lives, and Open shows it. A Markdown file with links or tags offers Connections or Tags once, as one notice, and a program that calls while Connected programs is off offers that app once; Settings, Apps has one switch for these offers.

- **Chat**: a pane beside the file (`Cmd+Shift+A`) with the open file attached; `@` attaches another. Only attached files are sent, and the send dialog names the host and the byte count first. A proposed edit shows as a line diff with Apply and Discard. Conversations are kept and can be renamed or deleted.
- **Rewrite**: proofread, rephrase, polish, improve prompt (told to keep `{{placeholders}}` as written), or your own instruction, on a selection.
- **Connected programs**: an MCP server, `writ mcp`, that another program starts over stdio. Seventeen tools: ten that read (list_notes, search_notes, read_note, note_links, note_backlinks, note_properties, note_tags, folder_tags, open_in_writ, reveal), six that write (write_note, create_note, rename_note, append_note, replace_in_note, today) and one that sees the file open in Writ (get_context), with separate read, write and open-file switches per program and nothing that deletes. append_note adds to the end of a file and replace_in_note swaps one passage found once, so a program changes part of a file without sending it back; open_in_writ shows a file in the window and reveal shows it at a heading. get_context answers the file in front and the text selected in it, only for a program whose Open file switch is on, and each answer is a line in Program activity. A program's change to an open file lands in its tab marked, keeping the caret, until you edit or switch tabs, and one Cmd+Z takes it back. With Ask first on, each write from that program waits in Writ with the change shown until you choose Apply or Discard, and nothing is written otherwise. Each file, and an index of every file with its title, tags and links, is also a resource, read from Writ's index when asked and never written into the folder. Paths in answers are relative to the folder, and the folder follows a change in Settings without a restart. Connect a program has a card per client: Add runs the own `mcp add` of Claude Code, Codex or Gemini CLI, opens the install link of Cursor, VS Code or LM Studio, or opens a bundle Claude Desktop installs, and Test counts the tools the command lists. A program waits under Waiting for you, on its card, in Settings and in a notice, until you approve it or don't allow it, and Program activity lists every call.
- **Connections**: a panel (`Cmd+Shift+\`) listing the files that link to the open one with the sentence each link sits in, its outline of headings with the one the cursor is under marked, and its frontmatter properties. A link to a file that does not exist yet has Create.
- **Graph**: a toolbar button and `Cmd+Option+G` (`Ctrl+Alt+G` on Windows and Linux) open a view of the whole folder, or of the files one link from the open one, with search, a legend of the colour per top-level folder, and zoom and fit buttons. A click selects a file; Enter or a double-click opens it. In the search, the arrow keys step through the matches and Enter opens the selected one or the first. With Connections on, its panel also draws the files around the open one.
- **Tags**: `#tags` and frontmatter tags in the sidebar with counts. Nested tags group under their parent, and selecting one filters the tree of Writ's folder.

Chat and Rewrite share one AI connection: Ollama, LM Studio, llama.cpp or Jan on this machine, Anthropic, OpenAI, Google Gemini, OpenRouter, Groq, DeepSeek, Mistral, xAI, Together or Fireworks with a key, or any OpenAI-compatible endpoint. The key goes in the system keychain on macOS and Windows, in the Secret Service (GNOME Keyring or KWallet) on Linux, and in memory for the session when no Secret Service answers. Writ asks before the first send to a host off your machine. On macOS 26, on a Mac that can run it, Rewrite can also use Apple's on-device model, listed under Built into macOS, with no key and no network request.

## More

- Spell check runs on this machine and is off until you turn it on. Code, URLs and identifiers stay unflagged.
- A watched folder (Settings, Integrations) opens new files as they arrive. See [adr/018-watch-inbox.md](adr/018-watch-inbox.md).
- The `writ` command opens files, folders and piped text from a terminal, and answers questions about the folder. See [cli-verbs.md](cli-verbs.md).
- On macOS and Windows, `writ://open?path=Plan.md&line=12` opens a file in Writ's folder at a line, from a browser or another program, and `writ://new?name=Plan` opens a new tab. A link opens only a file inside the folder and writes nothing. See [adr/052-writ-links.md](adr/052-writ-links.md).
- A folder written in Obsidian opens as it is: [what carries over](./importing-from-obsidian.md).
- No account and no telemetry.
- Each update is verified against Writ's signing key before it installs, and the automatic check can be turned off under Settings, About. See [adr/007-in-app-updater.md](adr/007-in-app-updater.md).
