# ADR-036: One read model over the index

## Status

Accepted. Amended 2026-09-12: `pathsForTag` names the tag and the tags under it (section 2).

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
- Only notes are nodes. The index also holds `.txt` and `.text`, which are
  findable and openable and are not notes a link can name, so an edge with
  either end outside the node set is dropped rather than drawn to a node that
  is not there.

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
- Nothing new is stored. Deleting `writ.db` still costs a walk and nothing
  else.
- Every index write Writ makes needs its event. A new command that writes the
  index announces `notes:indexed` once the write returns, or every surface
  keeps the rows from before it until another program touches the folder.
