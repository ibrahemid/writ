# ADR-041: Writ is a text app; Markdown is one format

## Status

Accepted, 2026-09-21. Lands before release 0.5.0.

Supersedes the positioning in [ADR-028](./028-files-are-the-only-copy.md) (the
context paragraph that makes Writ "for people who keep notes as ordinary files")
and the product direction in [ADR-029](./029-notes-on-disk-direction.md). Every
mechanism those records ship stands: files are the only copy of the text, the
notes folder, the write guard, the index, links, the graph, the harness. What
changes is what the product is called and what a fresh install shows.

Amends [ADR-039](./039-first-run-and-the-default-folder.md) in section 3: the
first launch asks one question and opens an untitled file rather than today's
note. Sections 1, 2, 4 and 5 of that record stand.

## Context

Between 0.4.0 and 1d30dc90 Writ grew links, a graph, connections, tags, a chat
pane, rewrite and an MCP server, and the copy on the site and in the README
followed each feature. Read together they describe a notes app: a place for a
folder of Markdown, positioned against Obsidian. The maintainer's direction of
2026-09-21 rejects that framing. The product that people install is a light
editor that opens any text file, edits and searches everything, and renders
Markdown as one of the formats it understands. A note-taking identity narrows
the audience to people leaving another notes app and puts every other text file
outside the product.

Three facts in the code carry that identity. A new file is always `.md`
(`crates/writ-storage/src/note_ops.rs` `NOTE_EXTENSION`, and its copies in
`note_host.rs`, `writ-cli` and `writ-mcp`), so somebody who opens Writ to write
a plain file gets Markdown whether they asked for it or not. A Markdown file
opens in a split with a rendered pane, which is the shape of a notes app, not an
editor. And chat, rewrite, links, the graph, connections and tags are on by
default, so a fresh install shows six features before the person has typed a
line.

## Decision

### 1. The line

Writ is "the only text app you need": light, opens any file, edits and
searches everything. Markdown is one of the formats it renders. The words
`notes app` and `Obsidian alternative` do not appear in Writ's copy; the
Obsidian guides stay as search-landing pages under Guides, off the main
navigation. The word `note` stays where it names the kind of file it names, and
in identifiers, since nothing about the file changed.

### 2. The default format is a choice, and the choice is `.txt`

`[files] default_extension` is `txt` or `md`, default `txt`. It is the one rule
for the extension of every file Writ mints: a new file (Cmd+N and the palette),
Today's Note, the file the `writ` command writes from piped stdin, the file a
connected program creates, and the file the first launch opens. Every site that
used to hard-code `md` reads the config instead.

A file keeps its own extension for the rest of its life. A rename, including
the one a first line offers, changes the stem and keeps the extension, so
`Untitled.txt` becomes `Groceries.txt` and never `Groceries.md`.

A link target is the one mint that does not follow the config. A note created
by following a `[[wikilink]]` is `.md`, because links resolve to Markdown and a
`.txt` target would be unreachable from the link that made it.

`.md` files behave exactly as before this record: the index reads their links,
link completion offers them, the preview renders them, the graph draws them. A
`.txt` file gets the plain editor: no link completion, no preview, no place in
the graph. Full-text search covers both, as it did before this record.

### 3. The first launch asks one question

The first launch shows one screen before the first file: two choices side by
side, "Plain text (.txt)" and "Markdown (.md)", one sentence each, and a
Continue button. The flow is a sequence of steps so a later record can add a
step after it (ADR-042 is reserved for the Apps step). Continue records the
choice as `[files] default_extension`, writes the config, and opens the first
file, an untitled file in the chosen format. Nothing is written until Continue,
so a person who quits on the screen is asked again next launch, and a person
whose config exists is never asked. The line under the cursor from ADR-039
section 4 stays and is dismissed the same way.

Today's Note is no longer the first file. It stays in the File menu and mints
its dated name in the chosen format. Settings, Files, carries the same choice
as a row, so the answer can be changed without a second first launch.

### 4. Markdown opens in the source editor

`preview.default_layout_markdown` defaults to `source`. A config that names
`split` or `preview` keeps it. The layout toggle and Cmd+Shift+H are unchanged
by this record; the Markdown inline mode and the removal of Split for Markdown
are the next record's.

### 5. Links, graph, connections, tags, chat, rewrite and the MCP server are apps

Each is switched on in Settings, under one section named Apps, and is off in a
fresh config. An app that is off contributes nothing visible: no menu item, no
palette command, no sidebar section, no shortcut, no status-bar item. An
existing config keeps its current values. The plugin runtime stays deferred
([ADR-012](./012-composite-transforms.md)); the Apps section is the seam it
will plug into. The section and its onboarding step ship in the record after
this one; this record fixes what the section holds.

## Consequences

**Positive**

- A fresh install is an editor with a cursor. Everything that made it look
  like a notes app is one switch away and off.
- One config key answers "what extension does a new file get", and one
  function reads it, so the CLI, the MCP server and the app cannot drift.
- Nothing built since 0.4.0 is removed. Links, the graph, chat and the harness
  keep their records and their tests.

**Negative and risks**

- Somebody who chose `.txt` and later switches to `.md` has a folder of both.
  That is the intended state of a text app, and the index already treats a
  `.txt` file as a plain file, but the Finder view is mixed.
- The first launch now has one question. ADR-039 argued against any. The
  question has a default and one tap answers it; the argument against a
  folder picker (a decision nobody can make on the day they install) does not
  apply to a format that the person already knows they want.
- Copy across the site, the README, the store listings and the installer
  describes the notes app until the copy record lands. Release 0.5.0 waits for
  it.
