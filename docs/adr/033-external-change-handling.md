# ADR-033: How Writ handles a file changing outside it

## Status

Accepted. This record covers the watching side. The file-identity rules that
tell a move from a delete, and the policy for what a reload does to a document
somebody is typing into, are added to it as those land.

## Context

Notes live on disk and the file is the only copy (ADR-028 §1). That makes every
other program on the machine a writer: a sync client pulling a folder that moved
on while the laptop was shut, Obsidian saving the same note from a phone, a
`git checkout`, a plugin, another editor. A text editor that reads a file once
and then writes over whatever is there is the failure people describe when they
say a notes app "silently lost an edit".

What already existed before this record:

- A recursive watcher over the notes folder (`start_notes_watcher`), feeding the
  index. It knew nothing about open tabs.
- The write guard: a save that would land over a change Writ never read is
  refused, and the text is written beside the note instead
  (`write_guarded_by_stamp`, `StorageError::SourceChangedOnDisk`).
- The ignore stamps: Writ's own writes are recorded under a `source:` key built
  from the file's resolved path and fingerprinted by content, so a save does not
  come back as somebody else's edit.

What did not exist: any watch on a file opened from outside the notes folder, a
bound on what one storm could report, and anything to do when a folder cannot be
watched at all.

## Decision

### 1. Watch the parent folder, never the file

Every careful writer — Writ, git, most editors, most sync clients — replaces a
file by writing a sibling temp file and renaming it over the target. The rename
gives the file a new inode. A watch registered against the file is a watch
against the inode, so it stops reporting the moment the first careful write
lands, and does so silently.

Watching the folder survives that, and it is what
[notify](https://docs.rs/notify/latest/notify/) documents as the supported way
to follow a file.

The cost is that the folder reports every file in it. That is answered by rule 3.

### 2. One watch per folder, released by the last tab in it

Ten notes opened from one repository cost one watch. The count is the notes
themselves (`WatchedDir::notes`, a map from note id to file) rather than an
integer, so the same note asking twice cannot leak a reference and a close can
find its folder without the path the open used.

A file inside the notes folder adds nothing: `start_notes_watcher` already covers
that tree recursively, and a second watcher over it would report every change
twice on the folder most likely to be large. A new note is never a special case
for the same reason — a note reaches a file on its first save, and that file is
inside the notes folder.

Folders are taken up on every path that puts a source-backed tab on screen,
including the listing that restores tabs at launch. A tab nobody has brought to
the front is precisely the one sitting on a file that has had the longest to
change.

### 3. Two different budgets, because the two watchers fail differently

Task-level, this reads as one cap. It is two mechanisms, because the two
watchers have different worst cases and one mechanism would be wrong for one of
them.

**The notes watcher: a per-window cap with a sweep marker.** The number of
distinct paths it can report in one 500 ms window is unbounded — a sync catch-up
rewriting five hundred files is the ordinary case. `EmissionBudget`
(`writ_core::watcher::budget`) names the first nine changes in a window with
their own paths, then emits the notes root once as a sweep marker and drops the
rest. The index reconciles by walking the folder, which is what it would have
done with the five hundred paths anyway, at a fraction of the messages. A
catch-up long enough to span many windows sweeps once per two-second cooldown
rather than once per window, so a slow storm degrades to a handful of walks
rather than the same burst in slower motion.

The root is the marker because no file event can ever carry it: the classifier
drops anything that is not a regular file, and the root is a directory. The
listener tells one from the other by the path alone, with no second field to
keep in step (`notes_sweep_marker` / `is_notes_sweep_marker`).

The marker rides the same bus as every other notes change, so it also reaches
the webview as `writ://notes-changed`. Nothing on the frontend reads that event
today. Whatever adds the first listener has to apply `is_notes_sweep_marker`
before treating the path as a file, since this is the one event whose path is a
directory.

The budget is spent only on changes that survived classification. A burst of
Writ's own saves is suppressed before it reaches the budget, so it cannot make
the folder look like it moved.

**The open-file watcher: a per-note dedupe, and no sweep.** Here the paths are
filtered to the registry before anything else happens, so the number of distinct
things that can be reported is the number of open tabs. Naming each note at most
once per delivered batch is therefore both a hard cap and lossless — nothing is
dropped that anyone wanted. A sweep marker would be wrong for this watcher: it
means "walk the folder and reconcile the index", and the index does not cover
`~/Downloads`.

The registry filter is also what keeps the temp file beside every atomic write
out of the stream, along with editor swap files and a sync client's in-flight
copies. None of them is an open note, so none of them reaches the ignore set or
the bus.

### 4. Fall back to polling one folder at a time

Where the native backend refuses a folder — a network mount, a FileProvider
tree, a folder whose contents are not readable — that folder alone moves to
`notify::PollWatcher` with content comparison. Content comparison rather than
timestamps, because an unreliable mtime is exactly what those filesystems have,
and they are the reason the fallback exists.

Per folder, not per process: one unwatchable share must not put the rest of the
machine on a timer.

Whether the native backend refuses depends on the filesystem underneath, which
no test machine can be made to have. The choice therefore goes through a
`DirWatcher` trait, and the tests inject a backend that refuses and assert which
one was selected rather than asserting that polling works.

### 5. Stay on `notify_debouncer_mini`

`notify-debouncer-full` adds rename stitching by filesystem id on FSEvents and
Windows. Writ does not need it: a move is detected by reading file identity over
candidate paths, which works on the filesystems where rename cookies do not, and
degrades to an external modification where identity cannot be read at all. Rename
stitching would be a second, weaker source of the same answer, and one that
FSEvents itself degrades into a move-out plus a move-in under rapid renames.

### 6. Dirty means the document differs from its file

`editorStore.isDirty(id)`: the digest of the live editor document against the
digest its file was last known to hold, on a 150 ms idle debounce. Not whether
an autosave is queued.

The two disagree in both directions, and both directions lose text. A note whose
autosave landed a moment ago has an empty queue and is dirty again on the next
keystroke; deciding a reload on the queue replaces what was just typed. A note
whose save the write guard refused has its queue emptied on purpose and has
everything to lose; deciding on the queue reloads over it.

It fails closed. A note the store holds no record of reads dirty, and so does a
note whose file could not be described — missing, or bytes not on this machine.
The tab restored at launch and never brought to the front is the ordinary case
for the first, and it is exactly the tab a watcher is most likely to raise a
change for.

`src/__tests__/architecture/dirty-predicate-authority.test.ts` holds this: the
autosave queue has one reader, its own module.

Failing closed makes the backend's side of the question stricter, not looser.
Nothing may announce a change it cannot demonstrate, because the announcement
now reaches a predicate that will read an unknown note as unsaved and ask the
user whether to discard work. So `resync_open_buffer` reports only against a
digest Writ recorded from a read of its own; a note with no record is passed
over rather than treated as changed. `disk_hash_matches` keeps the opposite
reading, which is right for the guard it exists for: a save with nothing to
compare against is refused.

### 7. A change never reloads the document registry

Reloading the global registry recreates the always-mounted `writ-preview://`
iframe, and removing a loaded one hard-freezes the macOS webview (PR #127).
Every path here ends at `requestExternalReload`, which resets one editor's
content and nothing else.

The reload reads the file through Rust (`read_buffer_content`), so the editor
receives what the file holds rather than any copy Writ was keeping.

## Consequences

- Opening a file from `~/Downloads` puts a watch on `~/Downloads`. That is the
  behaviour the spec asks for and the reason the group exists; the alternative
  leaves every file opened from outside the notes folder on the pre-watch
  behaviour, which is where the silent overwrite lives.
- A folder neither backend will watch is recorded as unwatched rather than
  pretended about. The tab still works and still saves; the write guard is what
  protects it, and it always was.
- A large sync catch-up is reported as a fact about the folder rather than a
  list of files. The index walks it, which costs a read of every note. That is
  the same walk startup does, and it is cheaper than five hundred round trips
  through the frontend.
- Two watchers can report the same folder for a while after the notes folder
  moves, since folders already watched are left where they are. That costs a
  duplicate event and never a missed one, and it resolves as those tabs close.
