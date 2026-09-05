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
folder. Its doc comment names the four that deliberately do not.

Three of them put no file behind a tab: `save_note_copy_inner` writes a copy the
caller then opens through the open path, `create_buffer` makes a note with no
file, and `get_buffer` only reads a row.

The fourth does, and is the exception. `open_generated_document` gives a tab a
file under the data directory and does not follow it: the row is read-only, so
nothing can be written to it and nothing can be lost by a stale copy; the file
holds Writ's own output and is rewritten from that output on every open; and
the folder is one Writ writes into, so a watch there would report Writ's own
writes back to it. Reopening still resyncs through `resync_open_buffer`, which
is what tells an open tab that the regenerated file differs from what it shows.

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

### 11. A watcher does not hear its own reads

Classifying an event means looking at the file: the notes watcher hashes it,
the config watcher parses it, the inbox watcher fingerprints it. On Linux the
`notify` crate registers `IN_OPEN` on every watched directory, so each of those
reads is itself an event, which arrives in the next batch, is classified, is
read again, and so on until the settle window closes. FSEvents and
`ReadDirectoryChangesW` do not report reads, so the loop was invisible on macOS
and Windows and surfaced only in Linux CI as eleven identical `BufferExternal`
events for one rewrite.

The rule is in `writ-core` (`watcher/sighting.rs`): a watcher reports a
delivered event only when the file's metadata differs from the metadata it
recorded the last time it looked at that path. The sighting is length and
modification time from `fs::metadata`, which does not open the file and so
raises nothing. A look is remembered for `DEFAULT_SIGHTING_TTL` (5 s, sized
like the ignore stamp: one debounce window plus the read's round trip). The
adapter takes the sighting before any read (`look_at` in `handler.rs`) and
the notes, config and inbox watchers all pass through it. Debounce timings do
not change; a digest gate on `disk_hash` was rejected because the frontend
already makes that comparison (§6) and a second copy of the decision would
disagree with it eventually.

Three comparisons of file content are weighed in this record and only one is
kept, so it is worth saying which is which. The one rejected here would gate the
read loop on `disk_hash`, the value the frontend already holds. The one rejected
in §12 would corroborate a file's id with its bytes, which answers a different
question badly: two notes holding the same text corroborate each other. The one
§13 keeps compares a report against the digest Writ itself last read or wrote,
after this gate has already passed, and it decides whether a modification is
news rather than which file anything is.

### 12. A vanished file is told apart from a moved one by its identity

A watcher reports a rename as a removal at the old path and a creation at the
new one, and on the fallback backend it reports the removal alone. Path is
therefore no evidence of what happened. The filesystem's own id is: `dev` and
`ino` on Unix, `FILE_ID_INFO` on Windows. A file that moves keeps it; a file
deleted and recreated under the same name does not.

The id is read whenever the tab learns what its file is: when it is given one
(`follow_note_path`), after every write Writ lands, and on every change a
watcher reports that finds a file at the path. A report that the path is empty
reads nothing — there is nothing there to read — and the removal waits for a
delivery that might answer it (decision 14) rather than retiring the id the tab
is still holding. The sighting is the one that matters most. A
save-through-replace — a temporary file renamed over the target, which is how
vim, VS Code, git, rsync and every sync client write — leaves the path holding a
different file, and a tab still carrying the id from open would read its own
next rename as a deletion, mark itself removed and refuse every later save over
a file sitting at its new path. So `note_file_returned` re-reads on every
sighting, not only on the one that clears a removal mark, and `observe_file`
decides what to keep: what the filesystem answers replaces what was recorded,
and a refusal to answer keeps what was recorded rather than blanking it. On a
volume that answers for every file — every Unix one — that refusal needs a
dataless file or a path holding something that is not a file to reach at all;
it is kept because the alternative leaves an evicted note with no id, which is
the state this closes.

Reading the id is not atomic with recording it. It costs a syscall, and on a
volume with no id to give it costs the whole file, so it happens outside the
lock the record is kept under. A save can land its own fresher id in that
window, and the watcher thread would then write the id it read before the save
back over the one the save wrote — the same stale record, arrived at from a
race. `identity_to_keep` settles it: the id on record when the read started is
carried to the write, and a record that changed in between belongs to a writer
that wrote later, so its value stands.

The id is the only evidence a move is followed on. Three different things
leave a vanished file whose id no candidate carries, and all three read the
same way. A program that rewrites a file and then renames it inside one watcher
window produces a single event saying the path is empty: two writes in one
window are reported as one, so the rewrite is never reported, the id on record
is the one it retired, and no sighting can fix that after the fact. A file
dragged out of every watched folder carries its id away to a path no candidate
names. A file that was deleted has its id nowhere at all. All three read as the
removal below: the tab keeps the text it is holding, the event carries nothing
to replace it with, and the next save is refused. Opening the file where it is
now is the extra way out of the first two, which is what the person can see and
Writ cannot.

Bytes are the obvious thing to reach for and they cannot settle it. A rename
changes none of them, so the file at its new path does hold what the tab last
read — and so do a copy, a second note from the same template, a sync client's
conflicted copy, and the backup a script wrote a moment before deleting the
original. Nothing in the window separates those from the file itself. The
debouncer reports a path without the kind of event that produced it (decision
5), so a file that arrived in the window and a file that was merely touched in
it are one shape, and a copy and a rename are one shape. Counting the matches
does not rescue it: the window is not the world, so a single match inside it is
no evidence there is not another outside it, which is where the note's real
destination sits in exactly the case the count is asked about. The empty file
makes the shape plain — the bytes every empty file holds name every empty file
there is, in this window and outside it — and what separates that digest from
any other is only how many files happen to hold it, which is the number the
count cannot see. Following a match anyway put the tab on a file it had never
read, whose bytes then satisfy the write guard exactly, so the next save
replaced that file's content with no event and no error while the note's own
file kept the old text with nothing pointing at it.

What would settle it is evidence of the pairing rather than of the content:
that this path appeared as that one left. A debouncer that keeps notify's
rename kinds instead of flattening them carries that, and a move recognised
that way would be safe to follow. It is an open direction rather than a
decision. Until it is taken, a vanished file whose id nothing carries is gone
as far as Writ can tell, and saying so costs a re-attach the person can make
themselves instead of a file they never opened. Opening the file at its new
path makes a second tab with a new id beside the removed one. The removed tab
keeps its text and the path the file left until the person closes it or saves
that text as a copy, and nothing merges the two.

A path holding a directory holds no note, the same as a path holding nothing,
and reads as a file that went. Dropping the event for not being about a file
left the tab carrying the dead file's id and its next save coming back as a raw
`Is a directory` rather than saying the file is gone. The index and the
frontend are still told nothing — a folder is not a note change — so only the
tab on that exact path hears it.

`writ_core::notes::identity` decides and `src-tauri/src/watcher/identity.rs`
reads, which is the policy and mechanism split the rest of the watcher follows.
`classify_delete` compares the id the tab holds against the ids of the files the
same batch names, plus the note's own folder; a match is a move, no match is a
removal. The probe is a trait, so the verdict is tested on every platform
regardless of which one runs the test.

An inode number is only unique among the files that exist at one moment. ext4
hands a freed one straight back to the next file created, so a note deleted and
an unrelated note created in the same watcher window can carry one `dev` and
`ino`, and the id alone then says the deletion was a move onto somebody else's
file — the tab follows it and the next save writes over it. APFS does not reuse
numbers, which is why the case reached CI rather than a laptop. The id therefore
carries a third field: the file's birth time in nanoseconds. A rename leaves it
alone, which is what `ctime` does not, so it separates a recreated file from the
original without weakening the move detection the id exists for. Two known birth
times must be equal; a volume that reports none answers `None` for every file on
it, and the inode is then the whole of the answer, exactly as before.
`is_same_file` holds the rule, and `==` stays exact-value equality because
`identity_to_keep` compares two records of one read and wants nothing looser.

The birth time is read on Linux and nowhere else, and equality is what makes it
worth reading there. It is worth comparing only where two things hold at once:
an inode number can come back, so the field has work to do, and nothing can move
the value under a live file, so a mismatch means a different file. Linux is the
only platform where both hold. ext4, xfs and btrfs reuse inode numbers, and
`statx` reports a `btime` that no userspace call can set.

macOS holds neither. A live inode's birth time moves in both directions on APFS,
both measured: `touch -t 202001010000` pulls it back through the modification
time, and `SetFile -d` — `setattrlist` with `ATTR_CMN_CRTIME`, which is what
unarchivers and restore tools use — writes it forward. So a
mismatch there says nothing about which file this is, and no ordering of two
birth times helps: reading a lower value as the same file lets a reused number
that was backdated through, and reading a higher one as a different file refuses
every save over a note a restore tool touched. APFS also never hands an inode
number back out, so there is nothing on it for the field to separate. macOS
therefore fills in no birth time and is answered on the inode alone. HFS+ does
reuse numbers, and a notes folder on a legacy HFS+ volume is answered the same
way, which is what every volume was answered on before the field existed. NTFS
file ids carry their own sequence number and ReFS ids are 128 bits, and a
creation time can be set through `SetFileTime`, so Windows reads no birth time
either and is answered on the file id alone. `is_same_file` reads both ids by
the one rule, so a record that does carry a birth time is compared on it rather
than having it dropped.

The birth time is the whole of the fix on Linux and not more than it. Linux
stamps a new file from the coarse clock, so two files created inside one tick
share a birth time to the nanosecond: a delete followed by a create at the same
path in the same tick stays indistinguishable. Every real replacement is
separated anyway, because a replacement is a sibling renamed over the target and
the sibling was created while the original still held its inode. What was
rejected was corroborating the id with a content digest: it answers the wrong
question, since two notes holding the same text corroborate each other, and it
would cost a read of every candidate on a share for a question the id already
answers. The tests that prove the rule build the identities by hand rather than
reading them from the host, because whether a filesystem reuses inode numbers is
the host's business and the rule has to hold on all of them.

A volume with no id to give — FAT, exFAT, some SMB servers — gets a description
of the file instead, which cannot recognise it anywhere else. That is
deliberate: `is_durable` is false for it and the verdict degrades to an external
modification, which reloads or asks rather than guessing that an unrelated file
is the same note. A file whose bytes are not on this machine is left with no id
at all rather than described, because describing it means hashing it and hashing
it means making the sync provider fetch it (ADR-028 §5). Nothing is probed for
such a note: the verdict cannot come out any other way, and on those volumes a
probe reads the whole file, so one deletion in a folder of four thousand notes
would pull every one of them over a share for an answer already known.

A candidate is only ever a path a watcher covers — the batch is one watcher's
own window, and the folder is the one the tab's file left. That is what makes a
match safe to follow: the tab lands somewhere its changes still reach it. It is
also the rule for the one case where the same id is honestly at two paths at
once. A hard link is one file with two names, and deleting one of them deletes a
name rather than the file, so the id the tab holds is still on the surviving
name and `classify_delete` finds it there — the survivor is followed on the id
like every other move, not on the file's content. Reporting a removal would
refuse every later save over a file that exists. A survivor outside every
watched folder would be a removal instead, for the same reason a move out of
every watched folder is.

Which name it follows is ordered rather than left to the filesystem. The batch
comes before the folder listing, and inside each the candidates are sorted
lexically, so the same set of names answers the same way on any volume;
`read_dir` order is the volume's. A path under the notes folder sorts ahead of
the rest, because that is the one Writ keeps a note in, and that preference is a
textual test on the two paths: where the folder watch and the notes root name
the same directory differently it does not fire, and lexical order alone
decides. The listing is capped at 4096 entries, and past the cap which names are
in the set is the listing's answer.

A move repoints the tab and nothing else: no read, no reload, no prompt. Its
row, its title and its index rows go to the new path through
`store.rename_to_file`, and the folder watch is re-armed through decision 9's
single arming point. Putting a move through the dirty gate would offer to
discard unsaved text over a rename.

A move that could not be applied is not silence. The row can fail to follow — a
lock nothing holds any more, a row that would not read, a rename the store
refused, a destination no string can spell — and the tab then names a path its
file is not at. What is still true about that path is the removal, so that is
what the tab hears: it keeps its text and stops writing, rather than saving over
whatever turns up at the old path later. `MoveOutcome` is what carries the
difference, because the one case that hears nothing looks identical from a bare
`false` — a tab already on the destination, which is one move seen by both
watchers and told by the first.

A removal is not a save. The tab keeps its text, is marked, and no keystroke
writes it back: the backend refuses that write under `ERR_FILE_REMOVED_ON_DISK`
and the frontend stops queueing one. Recreating the file would put back what the
person deleted, and in a synced folder it would put it back on every device. A
file put back where it was — the Trash restore — re-attaches, because the id it
comes back with is the one the tab still holds.

What "keeps its text" costs is a place to keep it. The tab's text is then the
only copy of the note (ADR-028 §1), and the editor view is not a place for the
only copy of anything: Writ has one view, a tab switch rebuilds it from the
file, and the file is what is gone. So the window's editor store holds the text
of every note marked removed, handed to it by the view before the view is
replaced, and a load of such a note reads the store rather than disk. A read
that fails must never reach the document — the empty string it falls back to
overwrites both the file's text and every unsaved keystroke on top of it, which
is the deletion finishing the job.

The ways out are three, and they are the person's to pick: write the text to a
new file, put the file back at its own path, or close the tab and let it go. The
second is what an explicit save means here, so the save keystroke does it too;
it goes through the one command that does not refuse a removed note, the mark is
dropped only once the write lands, and a folder that is gone as well refuses it
under `ERR_FILE_MISSING` rather than silently. Autosave is not one of the three:
it is the keystroke, and it stays quiet.

### 13. A report carrying the bytes the tab already loaded is not a change

A watcher reports what the filesystem told it, and the filesystem tells it about
writes that happened before the tab existed. FSEvents coalesces and delivers on
its own schedule: the write that seeded a file can arrive after Writ has opened
and read it, and on a loaded machine it does. Passing that on shows the user an
external-change notice for the bytes in front of them, and on a dirty tab it
offers to discard their edits for a change nobody made.

What separates a real edit from a late delivery is the digest Writ recorded when
it last read or wrote the file. Equal digests are no news, whoever wrote them and
whenever the report arrived; different ones are the change. A tab with no digest
on record still gets the report, because silence would be a claim about bytes
nobody read. The exception is a file that had been marked gone and is at its path
again: the tab is refusing to save until it hears so, which makes the return the
news rather than the bytes. `modification_is_news` holds all three cases, and
both routes to a tab go through it — the open-file watcher and the notes watcher
— because one of them staying quiet is no use if the other one talks.

The comparison is against what Writ last read or wrote, not against what the tab
was last told, so a file written away from the loaded bytes and back to them
inside one session ends on silence. That is the right way round: the alternative
is reporting a change to a file that holds what the tab is displaying.

### 14. A removal is held for one more delivery window before it is announced

Decision 12 recognises a moved file among the paths a delivery named and the
folder it left. Both of those are what the watcher has at the moment the path is
found empty, and a rename does not have to put its two halves in one delivery.
`notify_debouncer_mini` closes a window on a deadline set by its first event and
never extends it (decision 5), so a rename that lands on that boundary arrives as
the old path going empty in one delivery and the new file appearing in the next.
Answering the first one on its own reads a move as a deletion, marks the tab off
a file that is sitting one folder away, and refuses every later save to it. On a
loaded machine that is not an edge case; it is the ordinary way a rename lands.

So a path that went empty is not announced when it is seen. It is held, and every
later delivery is a chance to answer it with the file's id at another path.
Nothing answers by the deadline and the removal is announced exactly as it was
before. The wait is one hold window, twice the debounce the watcher delivers on,
so the delivery that would carry the other half has a full window of its own to
arrive in.

The id is the only thing a later delivery answers with. Bytes are not evidence of
where a file went, for the reason decision 12 gives: two notes can hold the same
text, so a path that matches on content is a path that might be a stranger's
file, and the next save writes over it.

Three rules keep the wait honest.

**Only a removal something could answer is held.** With no id on record that
could recognise the file elsewhere, no later delivery can say anything the first
one did not, and the wait would be latency for a foregone conclusion. Those are
announced straight away, which is what a watcher with no application behind it
does for every removal it sees.

**The record is left alone until the announcement.** A tab is marked off its file
when it is told, not when the path is first found empty, so a removal that turns
out to be a move never marks anything. The same file back at its own path before
the deadline stops the wait rather than being announced behind the delivery that
put it back — the same file, read by its id: something else at the path is a
stranger, and a save that landed there is a new file too, so neither ends the
wait. Asking whether anything is at the path would end it for both, and for
Writ's own write it would end it on an event the ignore set has already dropped.

**Nothing writes to a held path.** Leaving the record alone is what makes the
hold work and is also what leaves nothing in it for a write to trip over, so the
write asks directly: a save for a note with a removal held waits for the answer,
and then lands at the new path a move has already moved the row to, or is
refused under `ERR_FILE_REMOVED_ON_DISK` exactly as a save after any
announcement. Without the wait a save inside the hold recreates the file its
person deleted, and against a rename it puts a second file at the emptied path
and cancels the hold, so the move is never announced at all. `RemovalHolds` in
`writ-core` holds that contract — the answers, the deadline a wait is bounded
by, and the rule that an answer is published only once the record agrees with it
— and the watcher publishes each answer at the moment it has applied it. A
waiter released any earlier reads the old path out of the row and writes there,
which is the failure it exists to stop.

**Resolving comes before expiry, and an answer is the batch's one message for
that note.** A delivery that answers a removal makes it a move however long it
waited, and the events in that same delivery must not send the note a second
message (decision 8's per-batch rule). The delivery carrying the second half of
a rename names the old path too, and that path on its own reads as a file that
went, so each answer's note goes into the delivery's `told` set before any of
its own events are read. The sighting record of decision 11 would usually drop
that second look as well, but the two guards are separate and the per-delivery
rule holds without it.

The thread waits on the deadline as well as on the next event, so a held removal
is announced on its own schedule instead of when some unrelated change happens to
arrive. `PendingRemovals` in `writ-core` holds the state machine — hold, resolve,
expire — and the watchers supply the one fact it reads: what the filesystem calls
each candidate. Every watcher in the process publishes its holds into one
`RemovalHolds`, because a save asks about a note and not about a watcher, and
two watchers holding one note are answered on the later of the two deadlines.

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
- A move is only followed where the watcher reports both halves of it. A file
  moved to a folder nothing watches is a removal to the tab, which keeps the
  text and says the file is gone; the person can write a copy or point the tab
  at the file again by opening it.
- The halves do not have to arrive in the same delivered window, which is what
  decision 14 buys, and the price is that a deletion reaches the tab a hold
  window after the file went. A save landing in that window waits for the hold
  to be answered before it writes anything, so the price is paid in latency on
  a save that hits it and never in a file: at most one hold window and the
  window the answer is delivered on, once, for a note whose file has just gone.
- Another program deleting the note's file and writing a new one at the same
  path, which is how some sync clients land an update, is a removal to the tab
  once the hold passes. The file the tab holds is gone, and the path alone
  cannot separate that from a save Writ let through or from a stranger's file,
  so the tab keeps its text and stops writing rather than binding itself to a
  file it never opened. Opening the file again puts the tab back on it.
- A watcher with no application behind it (`FileTracking::untracked()`) has no
  digest on record for any tab, so decision 13 cannot answer for it and every
  late delivery is reported. That is right for what it is — nothing has read
  anything — and it is why a test that drives the watchers without a state has
  to carry the record a tab would have.
- A file rewritten and renamed inside one watcher window reads as a removal:
  the rewrite retires the id nobody reported, so nothing carries it and nothing
  else names the file. The tab keeps its text, says the file is gone, and the
  ways out are a copy written as a new note and opening the file at its new
  path. The alternative is following a path on content two files can hold,
  which hands the tab a stranger's file and lets the next save write over it.
- A note's file replaced by a folder of the same name reads as a removal rather
  than as nothing at all. A save then says the file is gone instead of passing
  on `Is a directory`.
- Deleting one name of a hard-linked file moves the tab onto the name that is
  left, and later saves write there. Someone who keeps a note hard-linked into
  two folders and deletes one copy is editing the other afterwards, silently.
  That is the true answer — it is one file — and the alternative refuses saves
  to a file that is sitting right there.
- The `buffer:external` payload gained `moved` and renamed `deleted` to
  `removed`. The event name is unchanged, so a frontend that has not been
  updated branches on neither and does nothing, which is the safe direction; a
  round-trip test on each side is what keeps the three words the same three
  words.
