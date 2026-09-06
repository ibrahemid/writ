# ADR-039: The first launch asks nothing

## Status

Accepted.

## Context

Writ's audience is now people who write notes, not only developers, and most
of them arrive from a tool that asked them to make a decision before it would
show them anything. Obsidian opens on a vault picker. The most endorsed advice
in its own community is to not configure it, and the named failure mode is
structure paralysis: people who set out to design the right folder scheme and
never write the note.

A first launch therefore has one job — put a cursor in front of the person —
and every question it asks costs it that job. Three questions were on the
table: where the notes go, what the note is called, and whether the person
wants a tour. None of them has an answer only the person can give on the day
they install the app.

There is a fourth cost that is invisible until it fires. A folder inside
`~/Documents` is TCC-protected on macOS, so creating it raises "Writ would like
to access files in your Documents folder" before the person has seen the app
work. A permission prompt in front of an empty window is a question about a
product they have not used yet.

Renaming carries its own cost. A note named for today's date is a fine name
until the first line says what the note is, at which point the file name is
wrong. But a rename inside a synced folder is a delete plus a create on every
other machine, and version history is keyed by file identity plus path, so a
rename nobody asked for can drop a note out of one and confuse the other.

## Decision

### 1. The notes folder is `~/Writ`, and nothing asks about it

`writ_core::notes::DEFAULT_NOTES_FOLDER` names it and
`resolve_notes_root_from` resolves it: `WRIT_NOTES_DIR`, then `config.notes.root`,
then `<home>/Writ`. The home folder root is not TCC-protected, it is visible in
Finder, and it can be dragged into the Finder sidebar. `~/Documents/Writ` was
the other candidate and is rejected for the prompt it fires.

The folder is created by `AppState::initialize`, before anything can write into
it, so the containment check the write gate runs compares two canonical paths.
Moving the folder later is a Settings row, where a permission prompt is
expected because the person asked for it.

### 2. A first run is the absence of a config file

`writ_core::startup::is_first_run(config_exists)` is the whole rule, and
`AppState::initialize` asks it before it reads the config, because reading is
what a later launch has a file for. A flag inside the config cannot answer
this: it has to be written before it can be read, and the launch that writes it
is the one that needs the answer.

The consequence is deliberate. Somebody who quits before anything persists gets
a second first launch. Somebody upgrading never gets one, which is what matters:
the line under the cursor must not appear for a person who has been using Writ
for a year.

### 3. The first launch opens a note named for today

`writ_core::startup::dated_note_name(now)` is `YYYY-MM-DD.md` in the local
calendar day, built on `writ_core::notes::date_stem` so there is one date rule
in the codebase. Today's note (D1) calls the same function. The tab therefore
carries a date rather than `Untitled-1` or a `writ-<millis>` placeholder, and
the file is in the notes folder before the window is shown, not on the first
keystroke and not at quit.

`first_run::open_first_note` runs in Tauri's setup hook, before the window is
revealed, so the frontend's ordinary "open the last tab" path finds it. There
is no first-run branch in the frontend's boot.

### 4. One line, dismissed on the first keystroke, kept in the config

`Your notes are saved automatically to a folder you can open in Finder.` — with
`File Explorer` on Windows and `Files` on Linux, one string and one substituted
word from `writ_core::startup::file_manager_name`, chosen from the host's own
platform constant rather than from the engine's `navigator`.

The dismissal is `config.first_run.hint_dismissed`, not web storage: storage is
per webview, and a person who reinstalls into the same profile would be told
again. Rust writes the flag and the frontend moves its own copy of the config
with it, because Writ's config write is stamped into the watcher's ignore set
and nothing else would tell the frontend the file changed. The line is the only
thing the first launch says.

### 5. An unasked rename needs two facts to still be true

`writ_core::startup::retitle_answer(RetitleFacts)` is pure over
`(has_been_closed, watcher_events_seen)`. A note Writ minted is renamed from its
own first line without asking only while its tab has never been closed and
nothing outside Writ has been seen touching its path since it was created.
Once either goes, the rename becomes one row beside the note offering it, and
the person decides.

`src-tauri` supplies the facts, because only it can: the close path records
one, and the event bus records the other downstream of the watcher's ignore
set, which has already dropped every write Writ made. The frontend is not asked
for either, and it never counts events itself — a frontend count is a filtered
projection, and the retitle window is exactly the first seconds when the
frontend may not be listening yet.

Not every first line names a note. A frontmatter fence and a line holding a
wikilink both leave the note its date, because the first is the note's text not
having started and the second points at another note. Both are ordinary first
keystrokes for somebody arriving from Obsidian, and a rename nobody asked for
is the wrong place to guess.

Every rename runs through `commands::notes::rename_note_inner`, so it is
stamped into the ignore set and recorded on the row in one write, the same as a
rename from the menu. It happens at most once per note: the note leaves the
table the first time its first line is answered for.

### 6. All three platforms run the same first launch

macOS, Windows and Linux open the same window on the same note, show the same
line with their own file manager's name, and answer a first line the same way.
Only the packaging differs: the DMG window layout is macOS-only because only
macOS has a DMG.

## Consequences

- A person who installs Writ and quits without typing sees the line again on
  the next launch. The alternative — writing a config file at startup to mark
  the launch — makes every later launch depend on a write that can fail, to
  answer a question that a missing file already answers.
- A note whose first line changes an hour later keeps its name. The rename is
  offered once, at the first save that has a first line, and never again.
- `~/Writ` is a folder in the home directory that the person did not create,
  which some people will not want. It is one Settings row to move, and moving
  it takes the notes with it.
- The first-run note exists even for somebody who opens Writ once and never
  types. It is an empty dated note in a folder they were told about, which is
  the cost of not asking.
