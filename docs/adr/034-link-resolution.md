# ADR-034: One link policy, four consumers

## Status

Accepted.

## Context

A note links to another note by name. `[[Weekly review]]` has to find
`<notes>/**/Weekly review.md` without the user knowing where either file sits,
and it has to keep finding it after the file moves.

Four separate places need that answer:

- the index, which stores a `to_path` beside every link so backlinks are a
  query rather than a scan;
- the preview, which renders a wikilink as an anchor or as plain text;
- the editor, which decorates a link and opens it on Cmd-click;
- the command line, which prints a note's links as JSON.

Nothing forces those four to agree. Writ has been here before: consent had two
URL parsers (ADR-027) and they agreed until they did not. Four copies of a
resolution rule is worse, because the failure is silent — a link that opens one
note in the editor and renders another in the preview looks like two features
working.

The stakes are higher than a wrong preview. Rename propagation rewrites the
links pointing at a renamed note. A resolver that guesses when it is unsure
rewrites the wrong file, and the user's evidence that it happened is a note
whose text changed for no reason they can see.

The tables have been in the schema since migration 040 and only `files` and
`files_fts` were ever written.

## Decision

### 1. The policy is `writ_core::notes::links`, and it is pure

The syntax set, the folding rule, the ranking and the ambiguity rule are one
module in `writ-core`. It opens no file and reaches no database: `resolve`
takes the candidate paths from its caller. The index passes rows out of
`files`, the editor passes what the IPC hands it, the command line passes the
same. No consumer reimplements a rule, and a consumer that wants a different
answer has to change the module and face every other consumer's tests.

### 2. The syntax set

- `[[Note]]`
- `[[Note|alias]]` — the alias is taken at the **first** `|`
- `[[Note#Heading]]` — the heading is taken at the **first** `#` of what is
  left, so `[[Note#Heading|Label]]` and `[[Note|Label#not a heading]]` both read
  the way they look
- `[[folder/Note]]` — the folder is everything before the **last** `/`
- `[label](path)` where the path ends in `.md` or `.markdown`, or carries no
  extension at all

A URL, an image, and a same-document `#anchor` are not links to a note. Code
fences, inline code and the frontmatter block are not scanned, so a link inside
an example stays an example.

### 3. Resolution order, and where it stops

A target carrying a `/` must match the candidate's trailing folders. A bare
name matches on the file name alone: `Note.md` answers to `note`, and a file
with any other extension answers only to its whole name, so `[[list]]` does not
reach `list.txt` and `[[list.txt]]` does.

Every comparison goes through `name_key`: **NFC, then lowercase, on every
platform**. macOS hands back a decomposed file name and the link is typed
composed, so `Café` is otherwise unreachable from the link that names it; the
same holds for an Arabic name copied between two applications. Doing this on
every platform rather than only where the filesystem forces it is what keeps
the app answering the same way on all three.

Among the matches: fewest path segments first, then the deepest common ancestor
with the note the link is written in.

**Two matches that ordering does not separate are `Ambiguous`, and `Ambiguous`
is never collapsed to a best guess.** The candidates are returned in byte order
so the editor can list them; alphabetical order presents them and never picks
one. This is the sentence to read twice: the rename in the link-propagation
work refuses an ambiguous target outright, and the index stores `NULL` rather
than one of the candidates, precisely so that no code path can quietly turn
"one of these two" into "this one".

### 4. `to_path` is nullable and backfilled

A note is usually linked before it is written. That link is stored with
`to_path = NULL`, not dropped, and it is resolved when the note it names
arrives — on the next single-file index of that note, or at the end of the next
walk. A link whose note is gone loses its `to_path` on the same pass, and a
delete does it immediately rather than leaving a link that reads as resolved
and opens nothing.

**A save re-resolves only the names the saved note answers to.** A vault where
most links are broken is the normal case, and a save that re-resolved every
pending link in the database would do that work on the connection every save
queues behind. One note arriving or leaving can only change the links that
named *it* — a `to_path` is a function of the set of indexed paths and nothing
else, never of any note's text — so the backfill takes the folded name keys of
the file that moved.
A walk passes no keys and re-resolves everything, which is also where a file
that vanished while Writ was not running loses its target.

An ambiguous link is stored `NULL` too, and is re-resolved with the rest; it
becomes resolved as soon as a rename or a delete leaves one candidate.

### 5. The four tables are derived, and rebuilding them is a walk

`links`, `properties`, `tags` and `headings` hold nothing that is not in the
files. Empty them and reconcile, and they come back.

That takes work, because reconcile's whole design is to skip a file whose size
and mtime match its row — which every file does after the tables are emptied.
So a complete pass records how many derived rows it left and over how many
files, in `schema_meta`, and a pass that finds fewer rows over at least as many
files re-reads every file. Notes deleted outside Writ shrink both numbers and
are not mistaken for a drop.

Two limits, stated rather than discovered:

- An index written before this record existed has no census, so its first pass
  re-reads the folder once. That is the upgrade, and it is correct.
- Emptying *some* of the four tables is caught whenever it lowers the total,
  which it does in every case except emptying a table that was already empty.

### 6. Headings are matched by slug

`[[Note#Some Heading]]` is matched against `headings.slug`, which is the
GitHub anchor: lowercased, everything but letters, digits, `-` and `_` dropped,
spaces turned into `-`. A repeated heading text keeps the bare slug on its
first occurrence and gets `-1`, `-2` after that. One rule, so the editor's jump
and the preview's anchor land on the same line.

A heading a note does not have leaves the link resolved and the heading
unreported: the note opens, and the reader is not stopped by a typo in an
anchor.

### 7. Frontmatter is split in three places, on purpose

`writ-render` compiles to wasm for the site and depends on nothing else in the
workspace, so it carries its own splitter. The prompt stripper next door
returns the body only, and a property parser needs the block. So
`writ_core::notes::links::split_frontmatter` is a third implementation of one
rule, and `crates/writ-core/tests/frontmatter_divergence_tests.rs` runs all
three over one table of adversarial inputs and fails the moment any of them
moves. That is the same trade `writ_core::prompt::strip` already made, made
once more and tested harder.

## Consequences

- Four consumers, one rule. Changing what `[[…]]` means is one edit.
- A link the resolver is unsure about stays visibly unsure all the way to the
  user. That is more UI than a guess would need, and it is the only version
  that cannot rewrite the wrong file.
- Every save now writes four more tables. They are written in the transaction
  that writes the file row, through cached statements, and the 5,000-note
  reconcile budget covers a corpus that carries frontmatter, headings, tags and
  links.
- The index is still safe to delete. Nothing here is a source of truth; the
  files are (ADR-028).
