# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.7.0] - 2026-10-15

Files in other text encodings open and save in their own encoding, a capture window adds a line to Today's File from any app, and Rewrite can run on Apple Intelligence on macOS 26 or on a model elsewhere on the local network. Chat edits, renames and moves to the Trash can be undone, Settings has eight sections with a sentence under every row, a second launch brings the running window forward, and reading and saving files no longer hold up the window.

### Added

- Cmd+Shift+Option+Space (Ctrl+Shift+Alt+Space on Windows and Linux) opens a small note over any app; Escape adds what was typed to the end of Today's File, and on macOS and Windows the app you were in gets the keyboard back.
- Open at login in Settings, General starts Writ hidden when you log in.
- A file in another text encoding, such as Windows-1252, Shift JIS, GBK or UTF-16, opens as text and saves back in that encoding, and a save that would lose a character the encoding cannot hold is refused with the character named. The encoding in the status bar reopens the file in another one.
- On macOS 26, Rewrite can run on Apple Intelligence on the device with no key, listed under Built into macOS; Chat asks for another provider while it is chosen.
- llama.cpp and Jan join Ollama and LM Studio as runtimes on this machine, and Settings shows which of them is running.
- An OpenAI-compatible server at a private network address over `http`, such as Ollama on another computer, works once you allow that host in a sheet that names it and the local network; a saved key goes over `https` only.
- Chat's first-use card offers a runtime it finds running, as in Use Ollama, with a model picker and a key field.
- Apply on a chat proposal for a file in the notes folder offers Undo, on the card and in a toast, which puts the file back as it was.
- Rewrite marks the words it changed, and Cmd+Enter applies the result or, after a failure, tries again.
- Rename with Update links, Move to Trash, Close all and Clear Recent offer Undo in their toast; Undo after a rename restores the name and every link it rewrote.
- Versions compares a version with the current text, offers Undo after Restore, saves a version as a new file, and says why a file has none, as in "Files over 2 MB are not versioned."
- With no file open, the window lists New file, Open file…, Today's file, Open file by name and Command palette with their shortcuts, then recent files; the arrow keys and Enter run them.
- An empty new file shows a placeholder for its file type until the first key.
- Hovering a link names the click that opens it, or that creates the file when it does not exist yet.
- With spell check on, the status bar shows how many words are flagged, and the count opens the review.
- Tabs that do not fit fade at the edge, scroll with the wheel and list in a menu with their unsaved marks, and tabs with the same file name add the shortest folder that tells them apart, as in README api and README web.
- The large-file chip opens a menu with Wrap lines and Highlight syntax for that file, and a paste that takes a file past 5 MB offers Use large-file mode.
- An HTML file over the preview's size limit shows its size in the Preview pane with Switch to Source.
- A picture in the editor that is not downloaded from iCloud stays in the cloud and shows "not downloaded" with Download.
- The Help menu holds Command Palette…, Writ Help, Keyboard Shortcuts, What's New, Show Setup Again and Report an Issue, which opens the bug form with the version filled in.
- File has Open Folder… (Cmd+Option+O), Reopen Closed Tab (Cmd+Shift+T) and F2 for Rename File…; Edit has Search Everywhere…, the Transform actions and Copy as Prompt; View has Next Tab, Previous Tab and the editor font size items.
- On macOS, the Writ menu has Hide Writ, Hide Others and Show All, and the Window menu has Minimize, Zoom and Bring All to Front.
- The right-click menu on a selection lists the Transform actions, and Copy as prompt sits beside Copy.
- Copy files as prompt, in the palette, copies the file in front with the files it links to, its folder's files or a tag's files as one prompt, with a token estimate per file and in all, up to 100 files and 4 MB.
- Typing a file's name in the command palette lists the file under the commands, and both palettes list their prefixes along the bottom.
- The sidebar's search field searches Writ's folder and a folder opened with Open Folder, with each folder's matches under its name; typing opens the sidebar on the results, the arrow keys walk them, Enter opens one at its line, and Escape clears the field and returns to the file.
- The graph opens from a toolbar button or Cmd+Option+G (Ctrl+Alt+G on Windows and Linux), with a legend, Enter or a double-click to open a file, and trackpad pan and pinch zoom.
- Each app's row and setup card in Settings, Apps lists where the app lives, with Open while it is on; an off app's shortcut opens its row, and an app switched on opens once when Settings closes.
- A Markdown file with links or tags offers Connections or Tags once while they are off, as a notice with Turn on; Offer apps that are off in Settings, Apps stops these notices.
- On Windows the MSI puts `writ.exe` on PATH, and on Linux the AppImage installs the `writ` command into `~/.local/bin`.
- On Linux, an AI key is kept in the Secret Service (GNOME Keyring or KWallet), and for the session only when no Secret Service answers.
- A Writ installed by a package manager announces a new version with "Update with your package manager." and no Install button, and a Writ left running checks for updates every 24 hours while automatic checks are on.
- When Writ cannot start, a dialog names the step that failed, and a crash leaves a report in the logs folder with the code location and none of your text.
- A first load that fails shows an error screen with Try again.
- On Windows and Linux, the close button's tooltip says Writ keeps running and which shortcut brings it back.
- A Settings search with no match offers Search everywhere with the same words.
- `writ search`, `cat`, `ls`, `append`, `today` and `open` work from any terminal or agent, with `--json` and exit codes 0, 1 and 2; `writ --wait` returns when the tab closes, so it serves as a git or shell editor; `writ Plan.md:12` opens at the line; `--no-open` writes without a window.
- Settings, Apps has a card for Claude Code, Codex, Cursor, VS Code, Claude Desktop, Gemini CLI, LM Studio and any other client: Add connects Writ through the client's own command, a link or a bundle, Test reports the tool count, and the first call is approved in place. Writ never edits another app's config file.
- `writ://open?path=Plan.md&line=12` and `writ://new?name=…` links open a file at a line or start a new tab from a browser or another app; they never write.
- Programs can append to a file, replace one passage, reach today's file and open a file in Writ, and read an index of the folder.
- A program granted Open file can read the file and selection open in the window and point the window at a heading; every such read is listed in Program activity.
- A program's edit to an open file lands in the tab as one marked change that keeps the caret and undo, the status bar says who changed it, and each version names its writer. A per-program Ask first setting shows every write as a diff to apply or discard.
- Two panes side by side show two files, or the same file twice, each with its own cursor and scroll. Split right (Cmd+Shift+2), Close split (Cmd+Shift+1), Focus other pane (F6) and Move tab to other pane (Cmd+Shift+M) in the View menu; a tab can be dragged to the other strip; the split, the divider and each pane's front tab come back after a relaunch.
- Pipe tables, display math and `mermaid` fences draw in place in a Markdown file in Inline, and show their source while the selection touches them. Math is typeset off the page thread; a diagram's styling comes from the theme, never from the file.
- Headings fold from a chevron in the margin and code from the gutter, with Cmd+Option+[ and ] (Ctrl+Shift+[ and ] on Windows and Linux) at the cursor; folds survive Inline, Source and a tab switch.
- Settings, Editor, Reading width holds prose to a reading measure; code keeps the pane's width.

### Changed

- Settings has eight sections: General, Appearance, Editor, Preview, Shortcuts, Apps, Integrations and About. Folder and Default format are under General, Markdown opens in is under Editor, the watched folder, the `writ` command and default apps are under Integrations, updates are under About, and the AI connection is on the Chat and Rewrite pages under Apps.
- Every Settings row has one sentence under it, rows that change how text looks show a sample, and Help, Keyboard Shortcuts opens Settings, Shortcuts; closing Settings with unsaved shortcut changes asks "Discard your changes?".
- Messages across the app say what happened in one voice, and raw system error text no longer reaches them.
- Each tab keeps its undo history, selection and scroll when you switch away and back, and a reopened file returns to its cursor and scroll position.
- `[[` suggestions list open and recent files first, then the files in the current file's folder.
- Typing no longer shows autosave in the status bar, and Cmd+S shows "Saved"; a failed autosave tries again with growing waits.
- Error toasts stay until dismissed, and a toast with an action stays for 8 seconds.
- Menus, tooltips and the Windows and Linux menus show each command's current shortcut, and recording a chord the macOS menu bar uses, such as Cmd+Q, names the conflict.
- Show and hide Writ needs Cmd, Ctrl or Alt in its chord, with F13 to F24 allowed alone; a chord another app holds is named in the shortcut editor and the title bar, and a Wayland session shows the shortcut as unsupported.
- A transform acts on the selection, keeps text typed elsewhere in the file while it runs, and stops with a message when an edit touches the text it acts on.
- In the command palette, `#` and `:` act only as prefixes, and file names typed with no prefix list at most five files.
- The file tree draws only the rows on screen and is one Tab stop, with the arrow keys, Home, End and type-ahead; folders stay open across launches, and right-click on any row shows it in Finder or copies its path.
- Opening a second copy of Writ brings the running window forward, and `writ <file>` opens the file there.
- Search skips files not downloaded from iCloud and says how many, as in "1 file not downloaded, not searched", and renaming such a file no longer downloads it.
- An opened folder's watcher skips build and package folders, caps how many changes reach the window, and checks a network share by size and modification time without reading its files, so installing packages into an opened folder no longer stalls the window.
- Reading a large file, installing the `writ` command and database upkeep at launch no longer hold up the window, and saves, reads and renames of one file keep the order they began in.
- Spell check uses SCOWL dictionaries for each English variant and still accepts the words 0.6.0 did, such as configs and backends; Fix all pre-selects only mechanical fixes and spellings with a single suggestion one edit away.
- A Markdown file name turns `#`, `[`, `]` and `^`, which break links, into spaces, as in `C notes.md` for "C# notes".
- The token estimate counts Chinese, Japanese and Korean text as one token a character and Arabic as half.
- Moving the notes folder to another disk copies it into a staging folder, syncs it, renames it into place and then removes the old one, and a folder holding a file not downloaded from iCloud is refused before anything is copied.
- The Versions, Activity and Chat panels show a failure line with Try again.
- The `writ` command takes subcommands and exits with 2 on a usage error, and piped text with no title is named `writ-<yymmdd>-<hhmm>` and lands with LF line endings.
- A copy of Writ opened from Downloads on macOS asks to be moved to Applications before it installs the `writ` command, and connected programs are handed the installed command's path.
- Connected programs get paths relative to the folder, page through `list_notes` and `search_notes` with `next_cursor`, and receive lists and search hits as objects with structured content, so a client that parsed the old arrays needs an update.
- Allowing a program to write asks for confirmation in a system dialog, and a link in a chat reply shows its full address with Open and Cancel before the browser opens it.
- A crash in a renderer or the preview no longer closes the app, and the logs keep 14 daily files that name files without their folders.
- Chat takes the keyboard when it opens and returns it to the file on Escape.
- Sending with no key asks for the key in one sheet before anything asks to send to the host.
- While a reply writes a change, Chat shows "Writing a change to" the file in place of Thinking.
- A chat proposal whose file changed after it was written shows Ask again in place of Apply, instead of writing a conflict copy.
- A long conversation sends the latest turns that fit 8,192 tokens for a server on this machine or the local network and 128,000 for a hosted one, with a line for each earlier proposal; a note over the size limit is refused when attached, and attachments stop at 4 MB in all.
- Running a command from the palette no longer rewrites `config.toml`; the palette's recent commands live in Writ's database and keep their order through the update.
- `install.sh` checks the release's minisign signature with minisign or OpenSSL 3 and stops when it has neither unless `WRIT_INSTALL_SKIP_SIGNATURE=1` is set, and the AUR package checks the AppImage's signature.
- The Linux AppImage's third-party licenses list the system libraries it bundles, with an offer of their source.

### Fixed

- One invalid value in `config.toml` stopped Writ from starting; Writ starts now, names the key and its line in a notice, and leaves the file as it is until it reads clean.
- The window could rename a file outside the notes folder after its tab closed, and read a picture beside a note no tab held; access to a file outside the folder now ends when its last tab closes or History is cleared.
- A note holding `</note>` could end its text early in what Chat sent to the model; the framing holds for any text now.
- A program's write landing while Writ saved the same note could lose the program's text; the note and a dated copy keep both now.
- `create_note` given "Plan.md" made `Plan.md.md`; the extension a program types is kept now.
- Connected programs kept serving the old folder after it moved in Settings; they follow it without a restart now.
- Renaming a file to a name that differs only in case or Unicode form, as Notes.md to notes.md, left the name on disk unchanged; the new name reaches the disk now.
- Search missed Arabic written with harakat or other letter forms, and Chinese, Japanese or Korean words inside a longer run of text; both are found now, and quick open matches accented names, as Émile.md for "émile".
- Search missed text saved in the last two seconds; it waits for pending saves now.
- A Markdown note with one very long line, such as a pasted base64 image, lost its formatting throughout; the long-line limits apply to that line only now.
- A picture over 2 MB in the editor showed a caption in its place; pictures up to 16 MB draw now, and typing inside an image link looks the file up once you pause.
- Shortcuts did nothing on a non-Latin keyboard layout such as Arabic or Russian; letters and digits match by key position now.
- On macOS, Control chords ran Writ's Cmd shortcuts, so Ctrl+N made a new file; in the editor Ctrl+N moves down a line and Ctrl+K deletes to the end of the line now.
- A rebound shortcut kept its old chord in the menu bar and the Windows and Linux menus, where the old chord still ran the command; the menus show and run the new one now.
- Enter that confirmed an input method's conversion in Chat, Rewrite or a rename also sent or committed the text; it only confirms the conversion now.
- Show and hide Writ accepted a single key such as A, a failed rebind left it with no shortcut, and Reset went back to the last saved chord; the rule is stated, the old chord stays until the new one registers, and Reset restores Cmd+Shift+Space.
- Ctrl+Shift+\ did not toggle Connections on Windows and Linux; it does now.
- Closing could wait without end on a save that never answered; it waits a set time now.
- One unreadable entry made the sidebar fail to list its folder; the folder lists and says what it could not read.
- Moving the notes folder to another disk reported a failure when the old folder could not be fully removed after the copy; it reports the move with what was left behind now.
- A long editing session could push the version from before it out of Revert To…; that version is kept now.
- Installing the `writ` command replaced any file at `/usr/local/bin/writ`; it replaces only Writ's own now.
- Piped text titled in Arabic or with accents was named with underscores, and two pipes at once could overwrite each other; titles keep their letters and a pipe no longer replaces another's file.
- Choosing Writ for a file type on Windows and Linux reported success and changed nothing; Linux registers Writ through `xdg-mime` now, and Windows opens Settings, Default apps.
- A chat draft was lost on New chat; it carries over now.

### Removed

- The `storage.path` key in `config.toml`, which Writ never used.
- The `[commands.usage]` table in `config.toml`; Writ moves an existing one into its database once.

## [0.6.0] - 2026-10-14

Text a save could not write now survives a crash, and the preview shows a document in a sandbox with its scripts off until they are allowed for that file. Typing and Find keep up in large files because a keystroke no longer copies the whole document, editor settings apply, and the sidebar shows the notes folder. Requests to AI providers carry only what each accepts, a program asking for files is announced where it can be answered, and releases are signed in their own jobs on current dependencies.

### Added

- When a save cannot reach the file (read-only, locked, unmounted, not downloaded, or timed out), the text is kept until a later save lands, and the next launch after a crash or quit writes it beside the note as a `(recovered …)` copy.
- A new note the notes folder will not take keeps its text in the tab under a message naming the file and the folder, with Save As….
- A previewed file's scripts stay off until they are allowed for that file, with the `Scripts off` field in the status bar or Preview: Toggle scripts for this file, and a file another program replaces has them turned off again.
- Change language… in the palette sets a file's syntax language and keeps it through closing and relaunching; Detect automatically goes back to the file name.
- A file over 50 MB shows a progress bar while it reads, after the question that names its size.
- Find says "Invalid pattern" for a regular expression that does not compile and "Pattern too complex" for one it gives up on.
- A program that asks Writ for files is announced in the window, as in "Claude Code wants to read your folder.", with Approve reading, Approve reading and writing and Don't allow.
- Settings, Apps, Connected programs lists programs waiting for an answer under Waiting for you, and the toolbar's programs mark shows a dot while one waits.
- View, Program Activity opens the activity panel while Connected programs is on.

### Changed

- Settings no longer has Allow HTML files to run their scripts, which was on by default; scripts are allowed per file.
- The restore toast at launch counts only the files whose text it wrote back.
- Find counts matches in large files without holding up typing, showing a running count with `+` until it settles, and searches again only the part that changed.
- A keystroke no longer copies, sends or hashes the whole document, so typing keeps up in large files with word, character and token counts on, and a file in large-file mode shows `≈` counts until the exact ones land.
- Spell check re-checks only the paragraph being edited.
- A file over 500 MiB is refused with a message naming its size and the limit, and a tab whose file grew past it shows that message with Try again.
- Tags, links, headings and properties come from `.md` and `.markdown` files only; the first launch after the update reads the notes folder once to drop the entries `.txt` files had added.
- The sidebar's Files section shows the notes folder whenever no other folder is open, with New file and Show folder in Finder while the folder is empty.
- Selecting a tag filters the notes folder's tree, with a `#tag` chip in the Files heading that clears it, and with Files folded or another folder open the tag lists its files under its own row.
- A provider with no model chosen starts on its default model when its list has it, then on a suggested model the list has, then on "No model" with the list open; Ollama or LM Studio with one model installed starts on that one.
- Default and suggested models are current for each provider, such as `gpt-6-luna` on OpenAI and `claude-sonnet-5-5` on Anthropic, and the OpenAI list shows chat models only.
- An error from a provider that rejects a parameter names it, as in "the model does not accept the max_tokens parameter".
- OpenRouter Connect answers stray requests to its local listener with a 404 and keeps waiting, and the browser page appears once the key is exchanged.
- Program activity describes each call in words, as in "Claude Code asked to open plan.md · Allowed", folds repeated reads into one row with a count, and shows known programs by their names.
- A program refused because Connected programs is off, or because it is waiting for approval, is told which and where in Settings to change it.
- tauri 2.12.3, tao 0.37, wry 0.57.0, harper 2.11 and rmcp 3.5, built with Rust 1.99.0.
- Release builds run without access to signing keys, signing happens in its own jobs, and every asset carries a build-provenance attestation that `gh attestation verify <asset> --repo ibrahemid/writ` checks.
- Each release attaches CycloneDX SBOMs for the app, the `writ` command and the frontend, and `SHA256SUMS.txt.minisig`, a minisign signature over the checksums made with the updater key.
- The Windows MSI is still unsigned.

### Fixed

- A network share that stopped answering held up saves in every other tab; other tabs keep saving now.
- A document in the preview could reach the network through a script, a resource hint, a meta refresh or a frame, and take a note's text with it; the preview now shows it in a sandboxed frame, and a file whose scripts are off sends nothing out.
- The preview was blank on Windows; HTML, Markdown, Mermaid diagrams and math draw there now.
- Find kept counting the previous tab after a tab switch; the count and highlights follow the tab in front now.
- Word wrap and Tab size in Settings, Editor had no effect; they apply to open files at once now.
- A `.txt` file holding a code fence was coloured as that language; `.txt` and `.text` files open as plain text now.
- Opening a file over 50 MB on Windows, or one with a colon in its name, asked about the wrong size and opened the wrong path; both open as asked now.
- A tab restored at launch, or a file reopened from History, read a file over 50 MB without asking; it asks first now.
- Cancel on the large-file question was reported as a failed open; it leaves no message now.
- A link to a missing file in the notes folder was answered as a file Writ may not open; it reads "Couldn't open spec.md: it does not exist." now.
- While a large notes folder was being read, as on the first launch after an update, quick open, `[[` completion, backlinks and Cmd+N waited behind the read; they answer during it now.
- Notes in a folder a `.gitignore` lists, or in `vendor`, `build` or `coverage`, were missing from search, links and backlinks; the notes folder now skips only `.git`, `.obsidian`, `.trash`, `.stfolder`, `.stversions` and temporary files.
- A download still arriving in the watched folder opened as a `.crdownload` tab; the finished file opens once instead.
- The outline, links, backlinks, graph and tags missed Writ's own saves, renames, new notes and deletes; they follow them now.
- Recent's times stopped advancing while the sidebar stayed open; they advance now.
- Chat and Rewrite on current OpenAI and Anthropic models failed with a 400 because Writ sent `max_tokens` or a temperature those models reject; each host now gets the parameters it accepts.
- Together's model list did not load; Together now runs at api.together.ai, and the first send after the update asks to allow that host.
- Stop before the first word of a reply did not end the request, and the next message was refused with "A reply is still arriving."; Stop ends it at once now.
- Applying a chat edit to a note with CRLF line endings wrote LF lines; the note keeps its line endings and final newline now.

## [0.5.1] - 2026-10-12

Two crashes and four ways typed text could miss its file, all found in 0.5.0. Connected programs now see every text file in the folder, and a write from one keeps the replaced text as a version.

### Fixed

- Renaming a file to a name in Arabic, Chinese, Cyrillic or any other non-ASCII script, from the tab, the sidebar or `writ rename`, closed the app. The same slice ran when the preview checked an image whose bytes begin with non-ASCII text.
- A trackpad drag onto the window that carried no file could close the app. The wry fix for it was pinned to an older wry and had stopped applying; it is applied again and a test keeps it applied.
- Closing the only tab, or opening another file, within a second of typing dropped the queued save. Restart to update did the same. The text is saved first now.
- A file Writ could not read (not UTF-8, locked, missing) opened as an empty document, and the next keystroke wrote that over the file. It now opens on a message naming the reason, with no editing.
- Typing in a file over 5 MB and switching tabs at once could write one file's text into the other.
- `config.toml` is replaced in one step, so a crash mid-write cannot leave it empty. A `config.toml` that is a hard link cannot be saved; a symlinked one is followed.
- Connected programs listed Markdown files only, so a plain-text folder looked empty to them. They now see every `.md`, `.markdown`, `.txt` and `.text` file.
- A connected program replacing a file it has not read is refused until it reads it or asks to overwrite, and the replaced text is kept as a version under File, Revert To….
- The Linux AppImage packed its launcher without the execute bit for other users, so it did not start under firejail.

### Changed

- rustls 0.23.45.

## [0.5.0] - 2026-09-26

Writ opens any text file, and a new file is plain text or Markdown, whichever the setup screen or Settings names. A Markdown file renders inline, with the markup shown on the line being edited. Chat, rewrite, connected programs, connections, the graph and tags are apps, each switched on under Settings, Apps, and all off in a fresh config. `[[Name]]` links one Markdown file to another, and the index behind links, tags and the `writ` command follows the folder as it changes. Nothing leaves the machine unless the client the user chose sends it. The app opens light with six accent colours and one menu bar on every platform.

### Added

- Links between notes. `[[Name]]` completes from the notes in the folder, opens the target on Cmd+click or Enter, offers to create a note that does not exist yet, and asks which note is meant when a name fits two. A Markdown link to a `.md` file resolves the same way. Renaming a note offers to rewrite the links that point at it, says how many before the rename, and lists any file it could not change.
- The index. Links, frontmatter properties, tags and headings are read from every note in the folder and follow files that change inside or outside Writ. The `writ` command answers from the same index with `links`, `backlinks`, `properties` and `tags`, gains `new`, `rename` and `trash`, and prints `--json` on request.
- Connections, a panel beside the note (Cmd+Shift+\): the notes that link here with the sentence each link sits in, the outline of headings, and the note's properties.
- Tags in the sidebar with counts. Nested tags group under their parent, and selecting a tag filters the notes list. A colour in pasted CSS and an anchor in pasted HTML are not read as tags.
- A graph of the notes around the open one, and a Graph view over the whole folder with search and one colour per top-level folder. Positions hold from one opening to the next.
- Inline Markdown. A Markdown file opens with headings at heading sizes, bold, italic, strikethrough and inline code styled, links with the address hidden, images under their line, task lists as checkboxes and fenced code highlighted in its language; the line being edited shows its markup. An Inline | Source switch in the status bar shows the raw text, and Settings, Preview sets which one a Markdown file opens in. Split and Preview stay for HTML and Mermaid files.
- The status bar shows the fields that fit the file type. Word, character and token counts sit behind a switch in Settings, Editor.
- A folder written in Obsidian opens as it is. Links by name, path, alias and heading, properties, tags and callouts carry over, and `.obsidian` and `.trash` contribute nothing. `docs/importing-from-obsidian.md` lists what carries over and what does not.
- A default format for new files. `[files] default_extension` is `txt` or `md`, and a new file (Cmd+N), Today's File, piped input to the `writ` command, `writ new` and a file a connected program creates all take it. The setup screen asks for it as its first step, and Settings, Files, Default format changes it later. The folder is `~/Writ`, and one line under the cursor says where it is.
- A file nobody named is called `writ-<yymmdd>-<hhmm>`, with `-2` for a second one in the same minute. A name that ends in `.md`, `.markdown`, `.txt` or `.text` sets the format, on a file named in the app and on a rename. Tabs, the file list and the palette hide `.md` and `.txt`; other extensions stay visible.
- An Apps section in Settings with one switch each for Chat, Rewrite, Connected programs, Connections, Graph and Tags. A fresh config has all six off; a config written before this keeps what it had, and a tags section hidden in the sidebar becomes Tags switched off. An app that is off has no menu item, palette command, shortcut, sidebar section or panel, and turning one on or off takes effect without a restart. Turning one off deletes nothing.
- An Apps step after the format step in the setup screen lists the six apps with a switch each. Settings, Apps opens the same screen later.
- One menu bar on macOS, Windows and Linux from a single command list, with Show Notes Folder, Recently Closed and Open Graph. The window hotkey can be changed in Settings; Save asks the OS first and reports the chord it actually holds.
- Interface text size in Settings, 12 to 22 px, scaling the sidebar, tabs, status bar, palette and settings together. The editor's own zoom stays separate.
- A light default that follows the system, with six accents (pine is the default) and the terminal presets kept as choices. The status bar is on by default. Sidebar, toolbar, tabs, palette, settings and dialogs are redrawn to one design, with the title bar drawn per platform.
- Save state per note: the status bar says saved only while the text matches the file, and a save that fails shows a bar under the note with the cause in a plain sentence.
- An MCP server, `writ mcp`, that another program starts over stdio. Reads: list, search, read, links, backlinks, properties, tags. Writes, under a separate permission: write, create and rename. It is off until enabled, and every client is refused until it is approved in Settings. It refuses a write that would overwrite a newer file when the client passes the hash it read. Nothing in it deletes a note. It opens no port and makes no request of its own.
- Connected programs in Settings, with read and write switches per client and a Forget control, and an Activity panel listing every call: time, client, tool, note, decision and byte count. The log holds no note text, prompts, replies or keys.
- A chat pane, off by default, on Cmd+Shift+A and in the View menu. It opens beside the note with that note attached, and `@` in the message attaches any other note. Only attached notes are sent; the send dialog names the host and the bytes first. Replies render as Markdown with a copy button on each code block, and a thinking state shows before the first word. A proposed edit shows as a line diff with Apply and Discard, never as two full texts; a note that changed since the proposal refuses the apply and leaves a conflict copy.
- Conversations are kept. Each chat is a file under the data folder, saved after every message, and the pane lists them by recency, reopens the last one, and can rename or delete one. The file holds no key and no note text. Stop ends a reply and keeps what arrived, Retry resends after an error, editing an earlier message resends from there, and the message box has a model picker for chat alone.
- One AI connection. Settings, AI holds one provider, one key and one model, and rewriting and chat both use it. On this machine: Ollama and LM Studio, found when they are running, no key needed. With a key: Anthropic, OpenAI, Google Gemini, OpenRouter, Groq, DeepSeek, Mistral, xAI, Together and Fireworks, each with a link to where its key is issued. Any other OpenAI-compatible endpoint by URL. The model list is fetched from the provider, and Check reports whether the host answers without showing its error text.
- OpenRouter can connect from a button. The browser opens, you approve, and the key lands in the keychain. Writ listens on this machine for that one answer and on nothing at any other time.

### Changed

- The app icon takes the accent the app opens with: the W is now pine on the same dark tile, in the dock, the taskbar, the site and the share image.
- When a note's file changes outside Writ, a clean tab takes the new text in one undoable step that keeps the cursor and scroll. A tab with unsaved edits shows a bar with three choices, and whichever side is not kept is written beside the note before anything is replaced. Files opened from outside the notes folder are watched the same way.
- A note keeps its tab when its file is moved, renamed or rewritten by another program. A note iCloud has not brought down yet opens on a download state naming the service instead of blocking the app.
- A save keeps the file's Finder tags, creation date, permissions and line endings, and saving unchanged text no longer rewrites the file.
- Every write to a note, from the editor, the `writ` command, a rename, a link rewrite, a connected program or a chat proposal, goes through one guarded path that refuses to overwrite a newer file and leaves a conflict copy.
- Sync clients' temp files and stubs never show up as notes. Conflict copies from Syncthing and Writ's own conflict files are listed and marked in the file tree.
- Sidebar sections fold on their heading and stay folded across launches, and Settings has a switch to hide each one. A nested tag is listed once, under its parent, by its last segment; selecting the parent lists the notes under it too. The sidebar no longer repeats the open notes; the tabs are that list.
- Settings rows are named in plain words and grouped by what people look for: Notes folder first, the data folder, watched folder and preview limits under Advanced. The sidebar search field says what it searches, and the history section is Recently closed.
- Delete Line moves to Cmd+Shift+K and Replace to Cmd+Option+F, freeing the chords macOS claims. Cmd+Option+S joins Cmd+\ for the sidebar. The preview split swap is Cmd+Shift+H.
- Choosing a light or dark preset pins that side; the System option in Settings is the way back to following the OS.
- The settings section "AI rewriting" is replaced by one AI connection under Settings, Apps, shown while Chat or Rewrite is on. An existing rewrite or chat setup carries over, and a key that either feature stored now serves both.
- Rewriting speaks to Anthropic as well as to OpenAI-compatible hosts, so every provider serves both features.

### Fixed

- A launch could leave Writ running with no window on screen and a dock icon that did nothing. One place in Rust now shows the window, a dock click brings back a window that is hidden, and a show that fails says so in the log.
- Name search ranked the whole absolute path, so a query could match the folders above the notes root. Ranking now runs over the path relative to the notes folder.
- On Windows a save that landed while another program held the file open failed with a permission error. It now retries and names the real cause.
- A database that lost its index tables is rebuilt on launch instead of failing every launch.
- Frontmatter `tags:` lines, including comma-separated ones, and the `tag:` key reach the index. Tag names are filed lowercased, so `#Project` and `#project` are one tag.
- A link rewrite that lost a race against an outside edit refused without leaving the losing side on disk. It now leaves a conflict copy like every other refusal.
- The theme store wrote eleven dead properties to the page on every boot, one of them as `[object Object]`.
- A rewrite whose provider returned an error ended the stream silently. The error is now shown.
- The `[[` completion stays installed across file type switches, so switching from a Markdown tab to a plain-text tab within 100 ms of typing no longer throws.
- Switching a Markdown file from Inline to Source keeps the caret line in view.

## [0.4.0] - 2026-09-05

Notes now live as Markdown files in a notes folder (`~/Writ` by default). The file on disk is the only copy of a note's text; Writ's database keeps derived data only. The first launch moves existing notes into the folder and keeps a rollback copy of the database for ten launches.

### Added

- A notes folder. New note (Cmd+N) creates a dated `.md` file at once; renaming a tab renames the file, Delete moves it to the system Trash, and Save a Copy writes an opened file into the folder.
- A Notes section in Settings with the folder's path, Show in Finder, Copy path and Move. Moving takes every note along and refuses a destination that would collide, naming the files.
- Open a note by name with Cmd+Shift+O. Cmd+O keeps the file dialog.
- The notes folder is watched and indexed, so search and open-by-name follow files that change, arrive or leave outside Writ.
- A report after the migration says how many notes became files, offers to move older archived notes into the folder, and points at anything it could not check.
- The `writ` command writes piped input as a note in the notes folder.
- Writ refuses to start with its data folder inside iCloud Drive, Dropbox, Google Drive, OneDrive or a Syncthing folder, names the service, and says where to point `WRIT_DATA_DIR`.

### Changed

- A save that would overwrite a newer version of the file on disk is refused, and the unsaved text is written beside the note as a dated conflict copy.
- YAML frontmatter is hidden in the preview and round-trips byte for byte on save.
- Autosave waits one second after the last keystroke, at most one write per note per second. Losing focus or hiding the window writes at once.
- Quitting from the menu, the Dock, or a logout writes pending notes before the app exits.
- `.obsidian`, `.trash`, `.stfolder` and `.stversions` are ignored in workspaces.

### Fixed

- Quitting inside the autosave window no longer drops the last second of typing.
- Saving `a/index.md` no longer hides a real change to `b/index.md`; internal writes are matched by full path.
- The session snapshot writes nothing when nothing changed.

### Removed

- The private copies of notes Writ kept under its data folder. Every note is its file.

## [0.3.5] - 2026-08-25

### Added

- `.sql` files highlight. The language was detected and named in the status bar, but no grammar was registered.

### Changed

- Failures that only reached the developer console now say so in the app: a config that will not read, a settings write that fails, a file that will not open, a palette section whose search throws. Log lines carry a fixed message and never a path, buffer text, or query.
- A workspace search that fails reports as failed instead of as no matches.

### Fixed

- Showing the sidebar keeps the caret in the editor; Cmd+\ no longer moves focus to the search box. A collapsed sidebar takes no clicks or focus.
- Escape closes a dialog after a click inside it. On macOS a click leaves nothing focused, so the key never reached the dialog.
- Context menus close on Escape or on typing, and use the readable foreground colour.
- The status bar drops fields as the window narrows instead of clipping them.
- Mermaid diagrams follow the document's light or dark theme, and an HTML preview without its own styles follows the light theme.
- The sidebar toggle in a fresh config defaults to Cmd+\, matching the shortcut 0.3.3 moved it to.
- The inbox header no longer reads "Inbox · Inbox" when the folder is named inbox.
- The AI connection row in Settings hides while a search filters to other sections.

## [0.3.4] - 2026-08-23

### Fixed

- The database no longer grows without limit. The recovery snapshot was rewritten every 30 seconds even when nothing had changed and the freed space was never reclaimed; snapshots now write only on change, and a bloated database is compacted once at startup.
- A save that fails no longer loses the text. The edit stays queued and retries on the next save, closing the tab asks before discarding it, and the message names the file and the reason.

## [0.3.3] - 2026-08-22

### Fixed

- Edits to a file opened from disk save back to that file. Since 0.3.0 they only reached Writ's internal copy, so the file on disk never changed.
- A saved file keeps its permissions, and a file whose contents changed on disk reloads when reopened.

### Changed

- Cmd+S saves the active tab right away. The sidebar toggle moves to Cmd+\.

## [0.3.2] - 2026-08-18

### Added

- Arabic and right-to-left writing. Each editor line takes its direction from its first strong character, so Arabic reads right-to-left and Latin left-to-right within the same document. Preview paragraphs, headings, lists, and tables resolve their direction the same way, while code blocks stay left-to-right. Arabic text renders in the platform's Arabic fonts instead of fallback glyphs.

## [0.3.1] - 2026-08-09

### Added

- A Writ menu on the Windows and Linux titlebar, opening the same actions the macOS menu bar carries: open file, new tab, close tab, the command palette, and the update check, each with its shortcut.
- Windows 11 snap layouts open from the titlebar's maximize button.

### Changed

- The Windows window controls follow the Windows 11 caption treatment: hover and pressed states, the caption red on close, and a visible keyboard focus ring. The maximize button reflects the live window state, showing restore while the window is maximized.

### Fixed

- Pressing the icon inside a titlebar button clicks the button instead of dragging the window.
- The Windows close button's tooltip says Hide, which is what the button does.

## [0.3.0] - 2026-07-30

### Added

- Search everywhere on `Cmd+Shift+F`: one palette over commands, settings, and every buffer, open or from history. With a workspace folder open it also matches file names and greps file contents on each query, streaming results as they arrive. Prefixes route the query: `>` commands, `#` content, `:` go to line.
- `Cmd+click` (`Ctrl` elsewhere) opens `http`, `https`, and `mailto` links from the editor in the default browser, underlining a link only while the modifier is held. A relative link that resolves inside the workspace opens in Writ instead.
- A link clicked in the preview names the host it points to and asks before opening. A scheme outside the allowed set states why it was refused and offers no way to open it.
- A Writ context menu in the editor, replacing the webview's. It offers what fits the moment: spelling corrections on a flagged word, link actions on a link, clipboard verbs, the rewrite actions when text is selected, and a workspace search seeded with the selection. Text fields get their own cut/copy/paste menu.
- Rewrite gained an "improve prompt" action, which rewrites the selection as a clearer instruction for a model and reproduces `{{placeholder}}` tokens untouched.
- Spelling corrections in place: double-click a flagged word to see its suggestions above it and click one to apply, or add that single word to the dictionary. Previously a word could only be fixed in bulk or ignored.
- Commands carry search keywords, so the palette finds them by terms that appear in neither their label nor their description.
- A "Third-party licences" row in Settings opens the bundled `THIRD-PARTY-NOTICES.md` in a buffer.
- A startup failure shows a dialog and writes a report file before the app exits. It previously aborted with no window and no trace of why.

### Changed

- Writ requires macOS 12 or later. The bundle previously declared 10.15, so a 0.2.0 install on macOS 11 or earlier must not update: macOS will refuse to launch this build and the update feed carries no OS check.
- The rewrite actions share a `Rewrite:` prefix and are findable as a group; searching the palette for "rewrite" previously returned only the custom action.
- Consent for a hosted provider is asked when the first rewrite runs, and names the host receiving the text. It was previously reachable only from a notice at the foot of the AI settings section, which left a configured provider failing with no way forward.
- Rewrite failures name the host and offer the setting that fixes them, and a failed rewrite can be retried without re-selecting the text.
- The landing page was rebuilt around sections that render as the page scrolls, and its demo window now runs the search palette. The changelog page lists every release instead of a single list of notes.
- The Linux install script checks the download against `SHA256SUMS.txt`, and against the minisign signature when minisign is on PATH. It installs the CLI as the `writ` command, so piping into `writ` reaches the CLI rather than the app.

### Fixed

- Menu items that opened a group did nothing when clicked and could not be reached by keyboard: Spelling settings, Close All Tabs, and Clear All History.
- Menus no longer open past the edge of the window. A menu that does not fit above its target flips below it, so corrections for a word near the top of a document are no longer pushed out of view.
- The OS keychain is consulted once per provider per session rather than on every rewrite, which on macOS raised a password prompt each time. A local endpoint never consults it.
- The endpoint reachability check is gated on the same per-host consent as a rewrite, and reports a missing consent as its result rather than as a connection failure. Picking a hosted preset previously sent a request with the API key attached before any dialog appeared. The consent text names the key and the check.
- A `.deb` install can update itself. The update feed serves the signed `.deb`; every Linux install was previously offered the AppImage, so the update failed after downloading it.
- The macOS `.pkg` is signed with a Developer ID Installer certificate, notarized, and stapled. A release that has the installer certificate set but produces an unsigned pkg now fails instead of publishing it.
- On the AUR package, `/usr/bin/writ` is the CLI, so piping and `--title` work. It previously pointed at the app, which reads argv as paths and ignores stdin. The desktop entry still launches the app.
- `writ <file>` opens the file in Writ on Linux and Windows. It previously went through the desktop default handler, which could open another editor, since Writ registers its file associations at Alternate rank.
- An empty piped payload opens nothing. It previously wrote a zero-byte file and opened a blank buffer.
- `writ --version` reports the installed version. It printed 0.1.0 on every release since the CLI landed.

## [0.2.0] - 2026-07-24

### Added

- Local spell check for text buffers: likely misspellings get a wavy underline while code, URLs, and links stay clear, with a status-bar item carrying the on/off switch, fix-all, and a per-word preview. Off by default and fully offline.
- Opt-in rewrite of selected text: proofread, rephrase, polish, or a custom instruction, streamed into a side-by-side original/result view before applying. Runs on a local model by default; hosted providers use a key stored in the OS keychain, and text is sent only when a rewrite is run.
- Line operations with editor-scoped shortcuts: duplicate, delete, move, and join lines, select line, insert a line above or below, select next occurrence, and add a cursor above or below.

### Changed

- Text transforms renamed to plain labels with descriptions of what each one does.
- The landing site's interactive window now runs the app's actual editor engine, sharing the editor modules, command tables, and theme tokens with the app. The site adds shortcut, terminal, and theme sections generated from the same sources, and the download section lists the Homebrew, winget, and curl installs.

### Fixed

- The AUR package installs the desktop entry and icons.
- The CLI install status no longer reports a dangling `writ` link as installed.

## [0.1.0] - 2026-07-06

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
