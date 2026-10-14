# ADR-042: Apps

## Status

Accepted, 2026-09-23. Carries out [ADR-041](./041-writ-is-a-text-app.md)
section 5 and adds the step ADR-041 section 3 reserved.

Section 4 amended 2026-10-13: Connected programs lists the programs waiting to
be approved and answers them in Settings, keeps a program turned down on its
list, and names MCP in its sentence.

Section 3 amended 2026-10-15: every app's chord answers while the app is off,
and each app row says where the app lives.

Section 4 amended 2026-10-15: Settings is eight sections named for the job,
Apps is a card and a page per app, and the terminal command, the file types
and the watched folder move to Integrations.

## Context

ADR-041 section 5 names chat, rewrite, the MCP server, connections, the graph
and tags as apps: each switched on in Settings, off in a fresh config, and
absent while off. Before this record only three of them had a switch
(`ai.chat.enabled`, `ai.rewrite.enabled`, `mcp.enabled`), tags could be hidden
as a sidebar section (`sidebar.hidden`), and connections and the graph were
always on. Off did not mean absent either: the View menu kept Toggle Chat, the
palette kept Chat and Activity, and the macOS menu bar was built once at
launch and never again, so no switch could take an item out of it.

## Decision

### 1. Six apps, one list

`writ_core::config::AppId` lists them in the order Settings draws them: Chat,
Rewrite, Connected programs, Connections, Graph, Tags.
`WritConfig::is_app_on` and `WritConfig::set_app` are the one reading and the
one writing of a switch. Chat, Rewrite and Connected programs keep the keys
they already had, because the chat command, the rewrite command and the MCP
server already refuse on them. Connections, Graph and Tags are the new
`[apps]` table.

### 2. What an existing config keeps

`AppsConfig::default()` is all off, and it is what `WritConfig::default()`
carries, so a missing config file is a fresh install. A config file with no
`[apps]` table was written before this record, and its `[apps]` reads as all
on, because connections and the graph were always on and tags was on unless
hidden. After the parse, `WritConfig::settle_apps` moves a `tags` entry in
`sidebar.hidden` onto `apps.tags = false` and drops it from the list, so tags
has one switch. It runs on every read and is idempotent.

`sidebar.hidden` only ever held folder, tags, inbox and recent, so it maps onto
Tags alone. Connections and the graph had no switch to carry over; they read
as on.

A fresh config also hides the recently closed section, so the sidebar shows
the file tree and search. The watched folder's section stays on: it draws
nothing until a watched folder is set under Advanced, and then it is what the
person set it for. That list is set in
`WritConfig::default()` rather than `SidebarConfig::default()`, because a file
with no `[sidebar]` table reads the latter and was written by somebody who saw
every section.

### 3. Off means absent, and on comes back without a restart

An app that is off has no menu item, palette command, shortcut, sidebar
section, toolbar button or panel.

- The macOS menu bar is rebuilt whenever the set of apps that are on changes:
  a Settings write, the first launch, and an edit to the file from outside.
  `menu.rs` answers which items a set of apps offers; the entries in
  `menu-commands.json` carry the app they belong to.
- The Windows and Linux menu is built from the command registry each time it
  opens, and skips a command whose app is off.
- Rewrite, Connections, Graph and Connected programs register their commands
  while on and unregister them while off (`defineAppCommands`), and the key
  map is rebuilt with them.
- Chat's command stays registered while chat is off and names its app, so the
  palette, both menus and the shortcut editor leave it out. Its chord then
  opens Settings, Apps with the Chat row marked, which is where the answer to
  "where is chat" is.
- Turning an app off closes what it had open (the chat pane, the graph, the
  Activity panel, the tag filter); the connections panel does not mount while
  off and comes back as it was. It deletes nothing: chat history, approved
  programs, panel widths, command usage and custom shortcuts are kept for the
  next time it is on.

Amended 2026-10-15. Every app's commands stay registered while it is off,
stamped with the app (`defineAppCommands`), which is what Chat alone did
before: the palette, both menus and the shortcut editor leave them out, and a
chord bound to one (⌘⇧\ for Connections, ⌘⌥G for Graph, a person's own chord
for a Rewrite action) opens Settings, Apps at that app's row, marked. Until
then ⌘⇧\ on a fresh install did nothing. Each row lists where the app lives
once on (toolbar, menu, chord or place, from `src/lib/apps.ts`) with Open
while it is on, and the setup cards list the same places with the app's icon.
The first switch-on of Chat, Connections, Graph or Tags from Settings opens
it once, when Settings closes, recorded in `[apps] opened`; the setup step
opens nothing, because the first file opens there.

### 4. Settings, Apps

Settings has eight sections, in this order: General, Editor, Appearance,
Apps, Integrations, Shortcuts, Advanced and About. Apps holds a card per app,
its switch and one sentence, and a page per app with what the app brings
while it is on: the AI connection (provider, address, key, model) on the Chat
and Rewrite pages, the chat model on Chat's, and the rows below on Connected
programs'. Settings search finds only rows that are drawn, reads every card
and page at once, and offers a section whose name matches.

Connected programs' sentence names MCP and two clients, Claude Code and
Cursor: a person who knows the protocol finds the app by its name, and one who
does not reads what it does. The first-run card shows the same sentence.

While Connected programs is on, its rows are, in order:

- Waiting for you: every program whose first call is waiting, with Approve
  reading, Approve reading and writing and Don't allow. This is the place the
  server's waiting refusal names, so the control is where the program sends
  the person. Its rows are the Activity window's own (`WaitingProgram.tsx`).
- The command to give a program, and what a program can do.
- Programs: every program decided on, approved or not allowed, with a switch
  per direction and Forget. Don't allow is stored as an entry that grants
  neither direction, so the program is answered as not allowed and does not
  wait again; switching a direction on there approves it.
- Program activity, which opens the Activity window.

Don't allow and Forget each leave a line with Undo while the list is on screen
and the program is still not allowed, or still forgotten. Undo of Don't allow
puts the program back to wait; Undo of Forget puts its entry back as it was.
The app also brings, while on, a notice when a program starts waiting while
Writ is open, with the same three answers; a programs mark in the toolbar that
opens Activity and carries a dot while a program waits; and View, Program
Activity (section 3).

The notice, the waiting list, Program activity and the server's refusals show
a known client by the name people know it by (`claude-code` as Claude Code,
`writ_core::activity::PROGRAM_NAMES`). The Programs list and the gate keep the
name as sent, so two names that read alike are still two programs.

Amended 2026-10-13. Until then the app kept its command, its per-program
switches and Forget, and an Activity link; a waiting program could be approved
only from the Activity window, the server's refusal pointed at a list that did
not hold it, and Forget had no undo.

Integrations holds the terminal command, the file types Writ opens and the
watched folder with its sidebar row. Advanced holds how an HTML file opens,
the two preview limits, Writ's data folder and how long versions are kept.
The tags sidebar row is the Tags switch.

Amended 2026-10-15. Until then Apps listed one row per app with the
connection and the program rows under their switches, and Advanced held the
terminal command, the versions row, the HTML row and the watched folder.

### 5. The first launch has two steps

`FirstRunStep` is `"format" | "apps"`. The format step's Continue moves to the
Apps step without writing; the Apps step lists the six apps with their sentence
and a switch each, all off, and its Continue writes the format and the
switches in one config write and opens the first file. A person who quits on
either step is asked both again. Settings, Apps opens the same screen later;
there its Continue writes the switches and opens nothing, and Cancel or Escape
leaves them as they were.

## Consequences

- A fresh install has no chat, AI, graph, tags or connections anywhere in the
  chrome.
- Somebody upgrading keeps everything they had, including a tags section they
  had hidden, which is now Tags switched off.
- The ADR-012 plugin runtime, when it lands, adds rows to the same list.
