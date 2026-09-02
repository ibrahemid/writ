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

A file inside the notes folder adds no *watch*: `start_notes_watcher` already
covers that tree recursively, and a second watcher over it would report every
change twice on the folder most likely to be large. It is still recorded, which
is the distinction the registry turns on: where a file lives (`Coverage`) is a
separate question from which tab holds it (`WatchedDir::notes`). Recording only
the folders this registry armed is what left a change inside the notes folder
with no way to find its tab — see decision 8. A new note is never a special case
for the same reason: a note reaches a file on its first save, and that file is
inside the notes folder.

Folders are taken up on every path that puts a source-backed tab on screen,
including the listing that restores tabs at launch. A tab nobody has brought to
the front is precisely the one sitting on a file that has had the longest to
change.

Two moves change which side of the line a folder sits on, and both release
before they record. A note whose file moves to another folder releases the
folder it left, or that watch outlives every tab in it and the file left behind
still resolves to a tab that has stopped editing it. Moving the notes folder
re-evaluates every recorded folder in both directions: one the notes root moved
onto gives up its own watch, and one the notes root moved away from takes a
watch of its own, without which every tab in the old notes folder went silent
until it was closed and reopened.

### 3. Two different budgets, because the two watchers fail differently

Task-level, this reads as one cap. It is two mechanisms, because the two
watchers have different worst cases and one mechanism would be wrong for one of
them.

**The notes watcher: a per-window ceiling with a sweep.** The number of distinct
paths it can report in one 500 ms window is unbounded — a sync catch-up
rewriting five hundred files is the ordinary case. `EmissionBudget`
(`writ_core::watcher::budget`) spends a window's ceiling of
`DEFAULT_EVENTS_PER_WINDOW` (ten) on the first nine changes by name and the
tenth on a `NotesSwept`, dropping the rest. Listeners reconcile: the index walks
the folder, which is what it would have done with the five hundred paths anyway,
at a fraction of the messages. A catch-up long enough to span many windows
sweeps once per two-second cooldown rather than once per window, so a slow storm
degrades to a handful of walks rather than the same burst in slower motion.

**The ceiling is per window, not per catch-up**, which is a deliberate departure
from W1's "at most 10 events for the whole catch-up". Each window refills, so
twenty windows of storm cost up to twenty times the ceiling less the sweeps the
cooldown swallows: 185 events on the default numbers, not 10. A per-catch-up cap
is not enforceable — nothing tells a watcher a catch-up has ended, only that no
change has arrived for a while, which is the window it already has — and a
budget that never refills would silently stop naming a person's own save for as
long as a sync client ran in the background. `DEFAULT_EVENTS_PER_WINDOW` is the
single constant, the arithmetic is in its doc comment, and
`a_catch_up_spanning_many_windows_costs_the_ceiling_once_per_window` pins the
exact counts.

The sweep is its own event rather than a `NotesChanged` carrying the root path.
The root worked while the only listener sat beside the sender in Rust; a
listener across the IPC boundary would have to fetch the notes root and
normalise it identically to recognise one. A variant makes the bridge translate
it and the frontend discriminate on `kind` (`notes_swept`, `writ://notes-swept`).

The ceiling governs what is said about the *folder*. Telling an open tab its own
file changed is outside it (decision 8).

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

The sweep obeys the same rule. `recheckOpenNotes` asks after one note at a time
and routes what it finds through `handleExternalEdit`; it never reloads the
registry, which is the one response to "everything may have changed" that would
freeze the webview.

### 8. A note in the notes folder reaches its tab through the notes watcher

Which folder a file happens to sit in must not change what its tab is told. The
open-file watcher deliberately arms nothing over the notes tree, so the notes
watcher is the only route in there, and it takes it: a classified `NotesChanged`
whose path is an open note also emits `BufferExternal` for that note
(`route_notes_change_to_open_tab`). Both watchers build that event with the same
functions (`open_note_modified` / `open_note_removed`), so the payload cannot
drift between them.

Without this the headline behaviour did not work on the default path. A change
to `~/Writ/today.md` reached the index and stopped; the tab kept showing text
the file no longer held, and its next autosave was refused by the W2 write
guard, which is a save failure in place of the change.

The notes watcher asks the registry rather than owning a map of its own
(`OpenNotes`, implemented by `Arc<Mutex<OpenFileRegistry>>`). One record of
which tab holds which file, two watchers reading it, and a seam a test can
supply a fixed answer through.

**Telling a tab is outside the emission ceiling.** The ceiling exists to bound
an unbounded number of distinct paths; tab messages are bounded by the number of
open tabs and deduplicated per delivered batch, so a folder being churned costs
at most one message per open tab per window. Capping them would mean a tab
losing the one change its user cares about because five hundred files they have
never opened moved in the same second.

**The sweep is the backstop.** When the folder changes faster than it can be
listed, no file is named, so the frontend re-checks every open note instead:
`recheckOpenNotes` asks Rust what each file holds and routes anything that
differs from what Writ recorded through the ordinary external-change path. Two
notes are passed over rather than reported. One with no recorded hash has not
been read this launch, and claiming a change would put a discard-your-work
prompt over a document nobody has typed into (decision 6). One whose file cannot
be described is either gone or not downloaded yet, and telling those apart is
the write guard's job.

### 9. One place arms the follow, and it names the paths that do not

`AppState::follow_note_path` is the only caller of `watch_parent_of`, and every
path that gives a tab a file goes through it: opening a file from outside the
notes folder, restoring a tab at launch or from history, creating a note,
giving a note its file on first save, renaming one, and moving the notes
folder. Its doc comment names the three that deliberately do not, because none
of them puts a file behind a tab: `save_note_copy_inner` writes a copy the
caller then opens through the open path, `create_buffer` makes a note with no
file, and `get_buffer` only reads a row.

Recording a file inside the notes folder matters as much as arming a watch
outside it. The registry is what answers "which tab holds this path", so a note
created with New Note, renamed, or given its file on first save and never
recorded was invisible to decision 8 for the rest of the session: the change
reached the index and stopped, exactly the failure that decision closes.

A rename releases the old folder and records the new path before it returns, so
a later note taking the freed name is never delivered to the renamed note's
tab.

### 10. A burst that stops is swept once more

Both coalescing rules drop work on the reasoning that something already under
way covers it, and both are wrong at the end of a burst.

A change arriving while a sweep still stands is dropped because the walk that
sweep started reads the folder as it finds it. That is true only for files the
walk has not reached yet. So the budget records that it dropped something and
names when the sweep for it is due (`owed_sweep_at`); the watcher thread waits
until then rather than blocking on its channel, and sweeps once
(`take_owed_sweep`). A sweep the budget emits in the meantime clears the debt,
so a storm that keeps going still costs one sweep per cooldown.

A sweep arriving while the index is walking was dropped for the same reason and
is wrong the same way. `ReconcileGate` keeps one walk at a time and remembers
that a request came in, so a burst during a walk costs exactly one walk after
it, whatever its length. Shutdown gives the gate back without running what it
owes.

One burst can pay both: a sweep the budget owes and a walk the gate owes. The
cost is one redundant walk of a folder that has gone quiet, which is the side
to err on.

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
- A catch-up spanning many windows costs many windows' worth of events, not one
  window's worth. The numbered criterion asked for ten across the whole
  catch-up; ten per window is what a watcher can actually enforce, and the
  difference is written down rather than implied.
- Moving the notes folder re-arms watches in both directions, so a folder can
  briefly be reported by two watchers or by none while the move lands. A
  duplicate event costs a redundant re-check; the direction that would have cost
  a missed one is the one this closes.
- A tab is followed from the moment it has a file, not from the moment it is
  opened from disk. Notes created and renamed in the session are the common
  case, and they were the ones decision 8 could not reach.
- A folder that falls quiet after a storm costs one more sweep and up to one
  more walk than the storm itself paid for. The alternative is an index that
  disagrees with the folder until something happens in it again, which may be
  the next launch.
