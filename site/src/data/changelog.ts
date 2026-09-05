export interface ChangelogNote {
  kind: 'added' | 'changed' | 'fixed' | 'removed';
  text: string;
}

export interface ChangelogRelease {
  version: string;
  date: string;
  label?: string;
  notes: ChangelogNote[];
}

export const releases: ChangelogRelease[] = [
  {
    version: "0.4.0",
    date: "2026-09-05",
    label: "Notes on disk",
    notes: [
      { kind: 'added', text: "notes live as Markdown files in a notes folder (~/Writ by default); the first launch moves existing notes there and keeps a rollback copy of the database for ten launches." },
      { kind: 'added', text: "New note creates a dated .md file at once; renaming a tab renames the file, Delete moves it to the Trash, Save a Copy writes an opened file into the folder." },
      { kind: 'added', text: "a Notes section in Settings: the folder's path, Show in Finder, Copy path and Move, which takes every note along." },
      { kind: 'added', text: "open a note by name with Cmd+Shift+O." },
      { kind: 'added', text: "the notes folder is watched and indexed, so search follows files that change outside Writ." },
      { kind: 'added', text: "the writ command writes piped input as a note in the notes folder." },
      { kind: 'added', text: "Writ refuses to start with its data folder inside iCloud Drive, Dropbox, Google Drive, OneDrive or a Syncthing folder, and says where to point WRIT_DATA_DIR." },
      { kind: 'changed', text: "a save that would overwrite a newer file on disk is refused and the unsaved text is written beside the note as a dated conflict copy." },
      { kind: 'changed', text: "YAML frontmatter is hidden in the preview and round-trips on save." },
      { kind: 'changed', text: "autosave waits one second after the last keystroke; losing focus, hiding the window or quitting writes at once." },
      { kind: 'fixed', text: "quitting inside the autosave window no longer drops the last second of typing." },
      { kind: 'fixed', text: "saving a/index.md no longer hides a real change to b/index.md." },
      { kind: 'removed', text: "the private copies of notes Writ kept under its data folder; every note is its file." },
    ],
  },
  {
    version: "0.3.5",
    date: "2026-08-25",
    notes: [
      { kind: 'added', text: ".sql files highlight; the language was detected and named in the status bar, but no grammar was registered." },
      { kind: 'changed', text: "failures that only reached the developer console now say so in the app: a config that will not read, a settings write that fails, a file that will not open, a palette section whose search throws." },
      { kind: 'changed', text: "a workspace search that fails reports as failed instead of as no matches." },
      { kind: 'fixed', text: "showing the sidebar keeps the caret in the editor, and a collapsed sidebar takes no clicks or focus." },
      { kind: 'fixed', text: "Escape closes a dialog after a click inside it." },
      { kind: 'fixed', text: "context menus close on Escape or on typing, and use the readable foreground colour." },
      { kind: 'fixed', text: "the status bar drops fields as the window narrows instead of clipping them." },
      { kind: 'fixed', text: "mermaid diagrams follow the document's light or dark theme, and an HTML preview without its own styles follows the light theme." },
      { kind: 'fixed', text: "the sidebar toggle in a fresh config defaults to Cmd+\\." },
      { kind: 'fixed', text: "the inbox header no longer reads \"Inbox · Inbox\"." },
      { kind: 'fixed', text: "the AI connection row in Settings hides while a search filters to other sections." },
    ],
  },
  {
    version: "0.3.4",
    date: "2026-08-22",
    notes: [
      { kind: 'fixed', text: "the database no longer grows without limit: recovery snapshots write only on change, and a bloated database is compacted once at startup." },
      { kind: 'fixed', text: "a save that fails keeps the text queued and retries; closing the tab asks before discarding it, and the message names the file and the reason." },
    ],
  },
  {
    version: "0.3.3",
    date: "2026-08-22",
    notes: [
      { kind: 'fixed', text: "edits to a file opened from disk save back to that file; since 0.3.0 they only reached Writ's internal copy." },
      { kind: 'fixed', text: "a saved file keeps its permissions, and a file whose contents changed on disk reloads when reopened." },
      { kind: 'changed', text: "Cmd+S saves the active tab right away; the sidebar toggle moves to Cmd+\\." },
    ],
  },
  {
    version: "0.3.2",
    date: "2026-08-18",
    notes: [
      { kind: 'added', text: "Arabic and right-to-left writing: each editor line takes its direction from its first strong character, so Arabic reads right-to-left and Latin left-to-right in the same document." },
      { kind: 'added', text: "preview paragraphs, headings, lists, and tables resolve their direction the same way; code blocks stay left-to-right." },
      { kind: 'added', text: "Arabic text renders in the platform's Arabic fonts instead of fallback glyphs." },
    ],
  },
  {
    version: "0.3.1",
    date: "2026-08-09",
    notes: [
      { kind: 'added', text: "a Writ menu on the Windows and Linux titlebar: open file, new tab, close tab, the command palette, and the update check, each with its shortcut." },
      { kind: 'added', text: "Windows 11 snap layouts open from the maximize button." },
      { kind: 'changed', text: "the Windows window controls follow the Windows 11 caption treatment, and the maximize button shows restore while the window is maximized." },
      { kind: 'changed', text: "the command palette is wider with a larger input and sizes to its results; modal shadows lighten across the app." },
      { kind: 'fixed', text: "pressing the icon inside a titlebar button clicks the button instead of dragging the window." },
      { kind: 'fixed', text: "closing the command palette no longer leaves a focus ring on the active tab." },
      { kind: 'fixed', text: "on Windows, the window no longer grows across launches or reopens larger than the screen, and a window closed maximized reopens maximized on the same monitor." },
    ],
  },
  {
    version: "0.3.0",
    date: "2026-07-30",
    notes: [
      { kind: 'added', text: "search everywhere on Cmd+Shift+F: one palette over commands, settings, and every buffer, open or from history. with a workspace folder open it also matches file names and greps file contents, streaming results as you type. prefixes route the query: > commands, # content, : go to line." },
      { kind: 'added', text: "Cmd+click (Ctrl elsewhere) opens http, https, and mailto links from the editor; a relative link that resolves inside the workspace opens in Writ. a link clicked in the preview names its host and asks before opening." },
      { kind: 'added', text: "a Writ context menu in the editor: spelling corrections on a flagged word, link actions, clipboard verbs, rewrite actions on a selection, and a workspace search seeded with it." },
      { kind: 'added', text: "spelling corrections in place: double-click a flagged word to pick a suggestion or add that word to the dictionary." },
      { kind: 'added', text: "third-party licences readable from Settings." },
      { kind: 'added', text: "a startup failure shows a dialog and writes a report file before the app exits, instead of exiting silently." },
      { kind: 'changed', text: "requires macOS 12 or later. on macOS 11 or earlier stay on 0.2.0: this build will not launch there." },
      { kind: 'changed', text: "the macOS installer is signed and notarized end to end, so Gatekeeper opens it without a warning." },
      { kind: 'changed', text: "the Linux install script verifies its download against the published checksums and installs the CLI as the writ command." },
      { kind: 'fixed', text: "deb installs update themselves: the update feed serves the signed deb instead of an AppImage the package manager rejects." },
      { kind: 'fixed', text: "checking that a hosted rewrite endpoint is reachable asks for the same per-host consent as rewriting before the API key is sent." },
      { kind: 'fixed', text: "writ file.rs opens Writ on Linux and Windows instead of the system default editor, and /usr/bin/writ on the AUR package is the CLI, so piping works." },
      { kind: 'fixed', text: "writ --version reports the installed version; it printed 0.1.0 on every release since the CLI landed." },
    ],
  },
  {
    version: "0.2.0",
    date: "2026-07-23",
    notes: [
      { kind: 'added', text: "local spell check: wavy underlines on likely misspellings, with fix all and a per-word preview from the status bar. off by default, and code, URLs, and links stay unflagged." },
      { kind: 'added', text: "rewrite a selection to proofread, rephrase, or polish it, streamed into a side-by-side view you approve before it applies. off until you turn it on, with a local model by default or a hosted provider whose key stays in the OS keychain." },
      { kind: 'added', text: "line operations: duplicate, delete, move, and join lines, select a line, insert one above or below, select the next occurrence, and add a cursor above or below." },
      { kind: 'added', text: "installs from the Homebrew tap and the winget package." },
      { kind: 'changed', text: "Cmd+E deletes a line; toggling inline code moves to Cmd+Shift+E." },
      { kind: 'changed', text: "editor shortcuts fire only while the editor has focus, instead of from any focused text field." },
      { kind: 'changed', text: "editor commands appear in the command palette and can be rebound in the shortcut editor." },
      { kind: 'changed', text: "text transforms carry plain labels and a sentence describing what each one does." },
      { kind: 'fixed', text: "a file created in the same moment the watched folder started being watched no longer goes unnoticed." },
      { kind: 'fixed', text: "the writ command no longer reports itself installed when its link points at an app bundle that moved or was deleted." },
      { kind: 'fixed', text: "the AUR package installs its launcher entry and icons." },
    ],
  },
  {
    version: "0.1.0",
    date: "2026-07-07",
    label: "first release",
    notes: [
      { kind: 'added', text: "split-pane live preview renders Markdown, HTML, Mermaid diagrams, and KaTeX math from bundled offline runtimes, with source and preview scroll in sync and find inside the preview." },
      { kind: 'added', text: "global hotkey brings the window up over anything: Cmd+Shift+Space on macOS, Ctrl+Shift+Space on Linux and Windows." },
      { kind: 'added', text: "tabbed CodeMirror 6 editor with language detection by extension and file content, live Markdown typography, and reopen-closed recovery." },
      { kind: 'added', text: "find and replace overlay with a live match count." },
      { kind: 'added', text: "full-text search across every buffer, with a results panel showing snippets and line numbers, backed by SQLite FTS5." },
      { kind: 'added', text: "command palette on double-tap Shift." },
      { kind: 'added', text: "settings window on Cmd+, with searchable settings and configurable shortcuts." },
      { kind: 'added', text: "editor and preview font zoom." },
      { kind: 'added', text: "status bar with language, encoding, and cursor position." },
      { kind: 'added', text: "prompt fill modal with placeholder variables, a live token estimate, and copy as prompt." },
      { kind: 'added', text: "text transforms: tidy whitespace, plus composable passes for trailing spaces, final newline, and punctuation spacing." },
      { kind: 'added', text: "open a folder as a workspace with a sidebar file tree; a watched folder lists new files and opens them as they arrive." },
      { kind: 'added', text: "large files open in a mode that keeps them responsive, and binary files open in a hex view." },
      { kind: 'added', text: "writ command-line tool for opening files from the terminal; set Writ as the default app on macOS for Markdown, plain text, config, and source files." },
      { kind: 'added', text: "app-wide light and dark themes, autosave with crash recovery, and self-update with signed manifests." },
      { kind: 'added', text: "builds for macOS, Windows, and Linux: .dmg, .msi, .deb, and AppImage." },
    ],
  },
];
