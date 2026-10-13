# ADR-036: One read model over the index

## Status

Accepted. Amended 2026-09-12: `pathsForTag` names the tag and the tags under it (section 2).
Amended 2026-10-13: commands read beside a running walk through read connections, one writer
is held for one file at a time and goes to a waiting write before the walk's next file, and the
walk leaves alone a path written beside it (section 7); facts and graph nodes come from Markdown
files only (section 3).

Amended 2026-10-13: Writ's own index writes are announced as `notes:indexed`, and every read lives in one keyed store that either event drops (section 4).

## Context

The index has held everything a note says about itself since migration 040 and
ADR-034 filled the last of it: `links`, `properties`, `tags` and `headings`,
keyed by the note's canonical path. Four surfaces are about to read it: a
panel beside the note, a tag list in the sidebar, a graph of the notes around
the open one, and a graph of the whole folder. None of them existed when the
tables were written.

Left alone, each of those surfaces would grow its own path to the rows. Three
of them want the same note's facts at the same moment, so each would call for
them separately: four tables, four calls, four answers that can disagree by a
few milliseconds. Two of them want the whole folder's links, which is a query
per note if nobody writes the query that reads the folder once. And every one
of them has to decide what "nothing here" looks like, which is where a list
that should render nothing grows a row saying it is empty.

The index itself is not in question. It is derived data that a walk rebuilds
and that is safe to delete (ADR-028), and the link rules that fill it are one
module with four consumers (ADR-034). What is missing is the shape those rows
come out in.

## Decision

### 1. The index is read-only to the UI

Nothing above the storage layer writes an index row. A surface that wants the
index to change changes the file, and the walk and the watcher put the row
where it belongs. The read model is a read model: `factsFor`, `allTags`,
`pathsForTag`, `graph`, and nothing that takes a value.

This is what keeps the index deletable. A panel that could edit a property
would be a second writer of a table whose first writer is a file on disk, and
the answer to "which one is right" would stop being "the file".

### 2. One call per note, not one per table

`note_facts` hands over a note's links, properties, tags and headings in one
answer, from one `NotesIndex::facts` call. The outline, the properties list and
the note's own tags are three readings of one row set, so they are one call.

Four calls would also be four moments. A note saved between the second and the
third would give a panel an outline from before the save beside properties from
after it, and nothing in the UI would look wrong.

`pathsForTag` is the fourth read: the notes one tag names, one statement over
`tags` joined to `files`, cached per tag beside the other three. A tag names
its own notes and the notes under it: `project` answers with the notes
carrying `#project` and the notes carrying `#project/alpha`, and
`project/alpha` answers with its own subtree. The tag list shows each tag
once, under its parent, so the parent row filters its family and a child row
filters its own tag; a sibling that merely shares a prefix (`projects`) is
outside the family. The match is still an equality on the folded string plus
one prefix test against the tag followed by a slash, never a `LIKE`.

A tag is filed lowercased, in the index and in every read over it. `#Project`
at the top of one note and `#project` in the middle of another are one tag with
one row and one count; two rows would be one pile split in half by a shift key.
The match is therefore exact again once both sides are folded, which keeps
`pathsForTag` a single indexed lookup rather than a scan.

### 3. The graph is a query over `links`, not a second store

`NotesIndex::graph` reads `files` once and `links` once, grouped. There is no
graph table, no cache on disk, no incremental maintenance. A graph is what the
links already say, asked in one statement per table.

What it leaves out is as much of the answer as what it puts in:

- A link with no `to_path` is a link that reached no one note. It is
  unresolved, or it names two notes and the resolver refused to pick. Either
  way there is no line to draw, so there is no edge. The note that wrote the
  link is still a node.
- A note linking to itself is a loop on one node and says nothing about the
  folder's shape, so it is dropped.
- Links written more than once between one pair collapse into a `count`. A note
  referenced twelve times is one edge with a weight, not twelve lines drawn
  over each other.
- Only Markdown files are nodes. The index also holds `.txt` and `.text`,
  which are findable and openable and are plain text: nothing in them is read
  as a link, a tag, a heading or a property (ADR-041 §2), so they draw no
  edge of their own, and an edge from a note to one is dropped rather than
  drawn to a node that is not there.

`folder` is the first path segment under the notes root, so a folder graph can
colour by it without a second read. A note in the root itself has no folder,
and so does a path the root does not contain: a note with nothing above it
belongs to no group rather than to an invented one.

The 5,000-note budget in ADR-028 section 7 covers it. A budget test builds that
corpus and reads the whole graph out of it, held to the query budget a keystroke
is held to rather than to the walk's.

### 4. Invalidation is an event, never a poll

Two events drop the read model. The watcher sends `writ://notes-changed` once
the index has taken a change the watcher did not skip: another program's
write, or a version restore, which Writ writes without an IgnoreSet entry. Rust
sends `writ://notes-indexed` after an index write Writ made itself. Creating a
note from a link also drops what links resolve to at once, so the editor
asking about that link again does not wait for the event.
There is no interval, no refresh on focus, no re-read on mount.

The watcher skips Writ's own writes (the IgnoreSet), so a save, a rename, a new
file or a delete never reaches it, and a read model that listened to the watcher
alone kept the rows from before every one of them. The command that writes the
index row announces it once the index call has returned, so a surface reading
on the event reads the new rows:

- a save, once the deferred reindex of ADR-020 lands, for a file in the notes
  folder, and a save the write guard stopped, naming the copy it set aside;
- a new file, a deleted file written back from its tab, and a copy into the
  folder: Save a copy, and Copy this version in the version panel;
- an answer to the change bar, at once, naming the copy it set aside and, for
  Keep mine, the file it rewrote, or the copy its save set aside when the
  guard stopped that save;
- a rename, naming the old file, the new one, and every file whose links the
  rename rewrote;
- a delete, once the row is forgotten;
- a walk that changed rows, and a move of the notes folder, each naming the
  notes folder itself, since any row in it may have moved.

One operation is one event, naming each file once. Opening a file records rows
for a file Writ did not change, and is not announced.

Neither event names the lists that changed: a tag added to one note moves the
folder's tag list, and a link added to one note moves the graph and the
backlink list of a note nowhere near it. So either event re-reads every read
something is showing, and drops every read nothing is showing.

Every read the window keeps lives in one store, keyed by what it reads: a
note's facts, the files linking to it, the folder's tags, one tag's files, the
graph, and what a link resolves to. A surface holds a key while it shows it and
hands the hold back when it stops. Holds are counted, so two surfaces showing
one tag share one read and the last hand-back drops it. Reading a key holds
nothing. Two surfaces asking for one key at the same moment wait on one call.

A burst of events, a sync client catching up or autosave settling in several
tabs, costs at most two rounds of reads: the first event starts a round, and
every event that arrives while it runs earns one more round after it, however
many there were. A read that lands after a newer one started is dropped, and a
link resolution read before a change and landing after it is handed to its
caller and not kept.

### 5. Zero rows is an empty list

Every read answers with a list, and a read that found nothing answers with an
empty one. There is no "no results" object, no null, no single row saying there
is nothing. A note the index does not hold reads the same as a note with
nothing in it, which is what both of them are to a reader.

The shape carries the rule so a surface cannot get it wrong: a section that
maps over an empty list renders nothing, and rendering nothing is what an empty
section is supposed to do.

### 6. Ambiguity is carried, never guessed

A target naming two notes reaches every surface as ambiguity. `resolve_note_link`
reports `ambiguous` with its candidates, `note_backlinks` lists the link under
both notes flagged, and the graph draws no edge at all. No consumer of the read
model picks a best candidate, and none of them is given one to pick from.

An edge drawn to a guess is the failure that has no symptom. The line looks
like every other line, and the folder shape it implies is one nobody wrote.

### 7. Commands read beside the walk

The index is written through one connection and read through others.
`NotesIndexStore` owns the writer. The walk, the deferred reindex behind a
save and the notes watcher all write through it, and each holds it for one
file: the walk reads a note with nothing held, takes the writer to record that
note's rows in one transaction, and lets it go before it reads the next. Every
command reads through a read connection of its own. The database runs in WAL
mode, where a reader sees the last committed state without waiting for the
writer, so quick open, `[[` completion, search, the backlink list, a link's
resolution and the graph answer while a walk of thousands of notes runs.

A walk lets go of the writer after each file and asks for it again at once,
and a lock gives no turn to the thread that has waited longest, so a write
beside a walk could lose the writer to it file after file: one Cmd+N write
waited while the walk began 65 other files. Every other write counts itself in
before it waits, and a walk takes the writer only while no write is counted
in. Cmd+N's write to the index waits for at most the one write the walk is
making when it asks, besides any other write already waiting: one note's rows,
which now and then include the full-text index merging its segments, or, at
the end of a complete walk, one batch of the closing link check that has
something to change. The longest gap between two files of a walk of 5,000
notes was 17 ms. Over three walks of 3,000 notes, the longest such wait was 12
ms, against 72 ms and 65 files before.

The app's own connection writes the same database: Cmd+N's row, and the stamp
every save leaves. It meets the walk at SQLite's write lock rather than at the
writer. SQLite's own handler waits longer after each refusal, up to 100 ms
between tries, so such a write kept waking up in the middle of a later file of
the walk: Cmd+N's row waited 295 ms while the walk went on through 293 more
entries. The app's connections to `writ.db` (its own, the layout store's and
the index writer's) try a refused write again every millisecond instead, for
the same five seconds. This bound is a likelihood, where the one above is a
guarantee: a try lands only when it falls between two of the walk's files, and
with one every millisecond a long run of misses is rare. Over three walks of
5,000 notes at a load average near 30 the row took at most 19 ms, and all of
Cmd+N at most 36 ms; with other builds saturating the machine the row reached
105 ms (ADR-028 section 7).

Because the writer is taken per file, a save, the watcher and a rename write
between two files of a walk, and what they write is newer than what the walk
saw. The store records every write it makes while a walk is under way, and the
walk leaves such a path alone: it writes no rows from a read older than the
write, and at its end it does not remove a file it did not find if the store
wrote that path after the walk started. A note that was away when the walk
reached it and came back before the walk ended keeps the row the write that
brought it back left. A folder move re-keys every row, so it counts as a write
of every path, and a walk that started before it writes nothing more. A rename
leaves the walk under way running.

Before this, one connection served both, and the walk held it from its first
file to its last. Every one of those commands waited for the walk to end: the
first command issued during a walk of 5,000 notes took 6,976 ms on a release
build, while the p99 the budget then measured read 2 ms.

The folder's names are kept in memory. A link resolves against the notes that
share its name, and reading every path from `files` to find them made one save
cost what the folder costs. The store reads the names once and changes them in
the transaction that adds or removes a path. That transaction also moves a
names version kept in `schema_meta`, and every write starts by comparing it with
the version the names were read at, holding the write lock so nothing can
commit in between. The names are read again only when another connection
added, removed or re-keyed a path; a commit that leaves the set alone, such as
the buffer store stamping a save, costs nothing. That is also how the read-only
store the MCP server and the `writ` command open keeps up with the app; it still
writes nothing (ADR-031 rule 1.3).

Every link row carries the folded name it names, `links.name_key` (migration
045). A note that arrives or leaves can change only the links that name it,
and it finds them through the index on that column; a save of a note the index
already holds changes no other link. The backlink list reads the links that
resolved to a note through the index on `links(to_path)` and the ones that name
it ambiguously through `name_key`. A search result's snippet is the fragment
the full-text index's own `snippet()` chose, with the matches its tokenizer
found marked, so a note's text does not leave the index to be shown and a
match that differs only by an accent is highlighted as well as found. The
marks are bytes UTF-8 never uses, so no character a note holds, a control
character included, is taken for one.

A complete walk ends by checking every stored link against the names, a batch
of rows at a time, with the writer taken only for a batch that has something
to change. It finds nothing unless a row was written some other way.

## Consequences

- One place to change what a surface reads. A fifth surface reads the same
  three functions, and a sixth table changes one query and one DTO.
- The panel, the tag list and both graphs cannot disagree about the same note.
  They read one answer.
- The graph is recomputed whenever a note changes rather than maintained. That
  is one read of two tables against a folder size the budget already covers,
  and it cannot go stale.
- A folder with an ambiguous name in it draws fewer edges than its links
  suggest. That is the honest count, and the panel says which links they are.
- Nothing is stored that a walk cannot rebuild. Deleting `writ.db` still costs
  a walk and nothing else: `links.name_key` is derived from `to_target` like
  the rest of the row, and the names version is a counter that starts again
  with the database.
- The names in memory cost the folder's paths once, folded once each. After
  another connection adds, removes or re-keys a note they are read again
  whole, the first time they are used. The app writes through its own store
  and does not pay this; the read-only store a connected program opens pays it
  once for the notes the app added or removed since that program's last call.
- Every index write Writ makes needs its event. A new command that writes the
  index announces `notes:indexed` once the write returns, or every surface
  keeps the rows from before it until another program touches the folder.
