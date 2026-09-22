# ADR-042: Apps

## Status

Accepted, 2026-09-23. Carries out [ADR-041](./041-writ-is-a-text-app.md)
section 5 and adds the step ADR-041 section 3 reserved.

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

A fresh config also hides the inbox and recently closed sections, so the
sidebar shows the file tree and search. That list is set in
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

### 4. Settings, Apps

The section sits after Files and holds one row per app, a switch and one
sentence. The AI connection (provider, address, key, model) is a row under
Apps shown while Chat or Rewrite is on; the chat model row shows with Chat.
Connected programs keeps its command, its per-program switches and Forget, and
the Activity link under its own row while it is on. Settings search finds only
rows that are drawn.

Rows a fresh install needs for neither an app nor its format move under
Advanced: the terminal command, how long versions are kept, the two HTML rows
and the watched folder's sidebar row. The tags sidebar row is the Tags switch.

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
