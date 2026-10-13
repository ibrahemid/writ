# ADR-044: The unsaved-text journal

## Status

Accepted, 2026-10-13. Ships in release 0.6.

Amends [ADR-028](./028-files-are-the-only-copy.md): the one copy of text outside a file that §1
allows is this journal rather than the recovery snapshot, and the heartbeat §8 slowed no longer
reads files.

## Context

Up to 0.5, crash recovery was a snapshot. A heartbeat thread woke every 120 s, took the store
mutex, read every open note's file from disk (`BufferStore::collect_buffer_contents`) and wrote the
contents into `session_snapshots` as an unclean snapshot. The quit wrote a clean one the same way.
After an unclean snapshot, the next launch wrote each entry back through
`BufferStore::restore_recovered_content`.

That design had three faults.

1. The snapshot held what the files held, not what the editor held. Text that never reached its
   file reached `writ.db` only when the frontend handed it over, at a quit or when its tab closed.
   A save is refused when the file is read-only, not downloaded, on a volume that was unmounted or
   that stopped answering. If Writ was then killed, crashed or lost power, everything typed since
   the last save that landed was gone. The audit recorded this as RC-04.
2. The heartbeat held the store mutex while it read every open file. A file on a network share
   whose server went away blocks the read, and every save and tab switch waits on that mutex. The
   save commands run on the main thread, so the window froze until the share answered. The quit
   read the same files. The audit recorded this as TS-04.
3. It kept a second copy of every open note's text in `writ.db`, which ADR-028 §1 allows only for
   text that has not reached a file. A snapshot whose `buffers` map failed to decode was dropped
   without a line in the log (`unwrap_or_default`), and the recovery module's documentation said
   the heartbeat ran every 30 s, when it had run every 120 s since 0.4 (RC-33).

## Decision

### 1. The journal

Migration 044 adds `unsaved_text`, with these columns:

- `id`: the entry's own id
- `buffer_id`: the note's row (`buffers.id`, `ON DELETE CASCADE`)
- `path`: the note's file when the row was written, for the log
- `text`: what the editor held
- `recorded_at`: RFC 3339
- `in_tab`: 1 while the entry is the text of the tab that typed it

An entry holds text a note's save could not write. Nothing else goes in it. While the tab that
typed the text is still the note's tab, the entry is that tab's: one per note at most (a unique
index on `buffer_id` where `in_tab = 1`), which the tab's saves replace or clear. Once the note's
tab stops being that tab, the entry is released (§3) and no save touches it. A note can hold any
number of released entries.

### 2. Who writes it

Every save the frontend makes (`save_buffer_content`, `save_buffer_content_unindexed`,
`restore_note_file`) goes through one write, `write_note_source`. What became of the text decides
what happens to the journal (`writ_core::recovery::journal_step`):

| The save | The journal |
|---|---|
| landed, or a note with no file and no text wrote nothing | the tab's entry is cleared |
| was refused over a file that changed under Writ, and the text was written beside the note | cleared: that copy is a file holding the text |
| reached no file (not downloaded, read-only, hard-linked, folder not writable, file or folder missing, timed out, any other I/O error, the file deleted outside Writ, the path not authorised, the first file of a new note not made) | the text is recorded as the tab's entry |
| was for a read-only note (a generated document, a binary file's view) | left alone: nothing in it was typed |

`StorageError::save_landing` reads a store failure, and the command answers for its own refusals.

The first save of a new note names its file and records the path on the note's row before it
writes. When that write makes nothing at the path, the path comes back off the row once the text
is recorded, so the row says again that the note has no file and the launch gives it one (§4). If
the text cannot be recorded, the path stays: a row with no file and no entry is an empty note,
which a new tab reuses and the next launch reclaims.

An answer to a file that changed outside Writ (`resolve_external_change`) writes the tab's text
too, to the note or beside it. Every answer that returns clears the tab's entry. One that fails
records the text.

`record_unsaved_notes` records too. The frontend calls it when a tab closes, when the window closes
and when Writ quits, with whatever it holds that is not known to be on disk. That includes text it
held back while a bar asked about the note's file, which no save carried.

A record replaces the tab's earlier text and leaves released entries alone. The same text and path
rewrite nothing, so a save retried with unchanged text costs a read. The record is written with
`synchronous = FULL` on that statement and the connection's own level is put back afterwards.
Under WAL at NORMAL, a commit survives the process being killed, but a power cut or an OS crash
can roll back the commits since the last checkpoint. A clear runs at NORMAL. If a power cut rolls a
clear back, the next launch writes a superseded text as a dated copy beside the note.

### 3. When it clears

A tab's entry clears when:

- the next save of that tab lands
- the person closes the tab with "Close and lose changes" (`discard_unsaved_text`), so text they
  chose to lose does not come back at the next launch. The note's released entries stay: the
  closing tab never held their text.
- the note's row is deleted (the cascade)

Closing a tab leaves its entry as it is, and the handover at the close (§2) replaces its text with
the editor's newest. The entry is released when the note's tab stops being the tab that typed it:

- the note comes back from history, by opening its file again or from the history list. Both go
  through `BufferStore::restore`. The new tab shows the file, so none of its saves carries the
  text, and none may clear it or replace it with the new tab's own.
- the app launches (§4): every tab comes up showing its file.

A released entry clears when a launch has written its text into the note's file or beside it, or
with the note's row. No save touches it.

A fileless note whose text is in the journal is not empty, so it is never reused for a new tab and
never reclaimed at startup (`list_unsaved_notes`). The notes migration's empty-row delete skips it
too.

### 4. Restore

Every launch reads the journal, whatever the last shutdown was. A quit with a failed save leaves
rows behind as surely as a crash does. The launch releases every entry first (§3), then restores
each, newest first, so a note with no file gets its newest text as that file and any older text
beside it. Each entry takes the path a snapshot entry took:

- `plan_recovery` withholds the write when the note's file was deleted
- a note with no file gets one minted, including a new note whose first file could not be made
  (§2). If the launch cannot make that file, because the path cannot be recorded or the write
  fails, a recorded path comes back off the row, the entry stays, and the text goes to the tab as
  a withheld note's does. A tab that came up empty would let its next save replace the text
  unseen. "Put the file back" mints the file, and so does the next launch.
- `restore_recovered_content` writes the text only into a path with no file, or into a file that
  already holds it. Any other file is left as it is and the text is written beside it as
  `<stem> (recovered YYYY-MM-DD HH.MM.SS)`.

The destination is the file the note's row names at the launch, not the journal's `path`, so a
note moved since its save failed keeps its new name. The text goes back with the note's own line
ending, as a save writes it. The journal holds the editor's text, whose breaks are LF whatever the
file uses.

What happens to each entry:

- written: removed from the journal
- withheld, because its file was deleted outside Writ (ADR-033 decision 15) or because the launch
  could not give a note with no file its first one: stays. The note's newest withheld text goes to
  its tab and becomes the tab's entry again, which putting the file back settles. Older text of
  that note stays released for a later launch.
- any other write that failed: stays released for the next launch, and is left out of the list
  the frontend counts for its toast, because nothing was restored

The toast itself is unchanged. It counts each text written, so a note restored from two entries
counts as two files.

### 5. Rows that do not decode

A row whose text is not UTF-8, or whose time does not parse, is logged by its rowid and left in
the table. The launch restores the rest. Removing the row is the one step that cannot be undone,
and it holds text that reached no file.

### 6. The heartbeat

The heartbeat still beats every 120 s (`heartbeat::SNAPSHOT_HEARTBEAT`). It has one job: record
once per process that the session is running. That record is an unclean marker in
`session_snapshots` with no text. Every beat takes the store, because the flag that says the
marker is written belongs to it. The first beat of a process writes the marker, one insert into
`writ.db`, while it holds the store; every later beat reads the flag and does no I/O. No beat
reads a note's file. The quit writes the clean marker as the last thing it writes, and that step
reads no file. The quit still runs the index updates that saves deferred (ADR-020), and each of
those reads its file.

The markers are diagnostics. The launch logs whether the last session ended without its clean
marker. Nothing is restored on their account.

### 7. Upgrading from 0.5

The latest unclean snapshot 0.5 left can hold text the quit was handed after a failed save, which
nothing else holds. Restart to update runs the quit, so anyone who updates while a save is failing
arrives in exactly that state.

Every launch starts by running `carry_over_legacy_snapshots`. For each snapshot row:

1. The latest row, if it is unclean, has its entries moved into the journal under 0.5's rule:
   only an entry newer than its note's row (`resolve_recovery`). An entry whose note has no row is
   dropped, as 0.5 dropped it. So is an entry for a read-only row, such as a generated document
   0.5 had open in a tab: nothing in it was typed, and the launch does not write into a read-only
   row, so the entry would never clear.
2. Every row that decodes has its text emptied out, which leaves `writ.db` with no copy of a
   file's text.
3. A row whose state or `buffers` map does not decode is logged and left exactly as it is. The
   prune skips any row that still holds text, so it is never pruned either.

Once no row holds text, the pass does nothing.

## Consequences

- Text from a save that did not land survives a crash, `kill -9` or a power cut from the moment
  the save command returns.
- The heartbeat reads no note file, so a share that stopped answering no longer holds the store
  through it.
- The index update a save defers (ADR-020) still reads the saved file while it holds the store, on
  its worker 2 s after the last save and at the quit. A share that stops answering inside that
  window after a save of one of its files holds the store until the read returns.
- `writ.db` holds note text only in the journal, for text a save could not write, and in any 0.5
  snapshot row that could not be decoded, which §7 keeps exactly as it is.
- The launch reads the file of each note the journal holds text for. If that file is on a share
  that has stopped answering, the launch waits for it. It already waits there for every note's
  file, journaled or not: the consistency check asks whether each row's file exists, and each
  row's path is resolved again for the write authority (`src-tauri/src/state.rs`).
- A save to the stalled share itself still holds the store and the main thread for as long as its
  write blocks. That is the save path's own I/O, which this record does not change. A tab open on
  the share stalls the window only if it saves.
- Text typed while a bar holds a note's saves (its file changed or was deleted outside Writ)
  reaches the journal when its tab closes or Writ quits, not as it is typed.
- A row that names a path with nothing at it reads as a note whose file was deleted, so its text
  goes to its tab as removed on disk rather than to a new file. A new note whose first save failed
  under 0.5 is left that way, as is one whose process was killed between minting its path and
  finishing the write. A new note whose first file the launch cannot make either comes up the same
  way: the bar says the file was deleted, which it never was, and putting the file back makes it.
- An entry whose restore failed for a note that has a file is not handed to its tab, which shows
  the file. The launch released it, so none of the tab's saves replaces or clears it, and the next
  launch tries again. 0.5 counted such text as restored and lost it at its next heartbeat.
- Text a closed tab handed over is not shown when its note is reopened in the same session. It
  comes back at the next launch, beside the file when the file has moved on, as 0.5 brought it back
  after a quit, and from the handover on it survives a crash too.
- A save clears its tab's entry by note. That assumes no write of the tab is still running when a
  later one lands or a handover arrives. The frontend's autosave runs one write per note at a time,
  and the close and quit handovers wait for its flush.
- FULL syncs with `fsync`, not `F_FULLFSYNC`, on macOS. ADR-028 §9 left that measurement open.
