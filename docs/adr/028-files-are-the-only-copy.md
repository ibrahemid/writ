# ADR-028: Files are the only copy of the text

## Status

Accepted, 2026-08-28. Supersedes [ADR-004](./004-sqlite-over-flat-files.md). Ships in release 0.4.

[ADR-020](./020-deferred-fts-reindex.md) and [ADR-024](./024-inbox-arrival-snapshot.md) stay in force. Section 12 records what each means after this change.

## Context

Every note in Writ exists twice. `BufferManager` mints a mirror filename `{id}.txt` for a new note
(`crates/writ-core/src/buffer/manager.rs:101`) and for a file opened from disk (`:146`).
`write_source_and_mirror` writes the source file and then the mirror on every save
(`crates/writ-storage/src/buffer_store.rs:428`, mirror at `:442`), and both save entry points route
through it (`:397`, `:416`). The editor reads the mirror, never the file (`read_content`,
`:248`); the source is consulted only on reopen, through `read_source_if_diverged` (`:455`).
The clean-shutdown and heartbeat snapshots read mirrors off disk rather than asking the frontend
for the live document (`collect_buffer_contents`, `:703`, called at `src-tauri/src/lib.rs:559`
and `:628`). A third copy of the text sits in FTS5
(`crates/writ-storage/migrations/001_initial.sql:19`), which the public privacy page states as
fact (`site/src/pages/privacy.astro:21-24`). A fourth is `~/.writ/piped/`, where the CLI writes
piped stdin (`crates/writ-cli/src/main.rs:110-114`, claimed at `site/src/pages/privacy.astro:30`).

That is ADR-004 working exactly as designed. Its reasoning was about structured state at scale:
indexed queries over buffer metadata, FTS5 over content, WAL crash safety, migrations. None of
those arguments require the database or a mirror to hold the text, and three of the four copies
exist only because the mirror was made authoritative for reads.

The product direction settled on 2026-08-25 makes the mirror untenable. Writ is for people who
keep notes as ordinary files in a folder they can open in Finder, and the promise is that Writ
does not lose them. A user who deletes `writ.db` must lose no note. A user whose folder is in
iCloud Drive or Dropbox must get their notes on the other machine. Neither is true while the
file on disk is a secondary artefact that a mirror is copied over.

The reversal has to be recorded rather than left implicit, because ADR-004 is still the standing
answer to "where does the text live" and the shipped public copy repeats it
(`README.md:56`, `:139`).

## Decision

### 1. The invariant

For any file in the notes folder or opened from disk, the file on disk is the only copy of the
text.

SQLite holds derived data only: the search index keyed by canonical path, per-file history
metadata, session and layout state, tab order. Deleting `writ.db` loses no note; it costs a
reindex and the session layout. The content-addressed history store that arrives with per-file
versions lives in Writ's data directory under `~/.writ`, never in the notes folder, so sync never
carries it and deleting the notes folder does not delete its own history.

One copy of text outside a file is legitimate: the recovery snapshot for text that has not
reached a file yet (`crates/writ-storage/src/recovery/`, `MAX_SNAPSHOTS` at
`crates/writ-core/src/recovery.rs:11`). That is a write-ahead buffer measured in seconds, not a
second source of truth.

Mirror files under `~/.writ/buffers/` are retired. `buffers_dir` (`src-tauri/src/state.rs:76`)
stops holding note text. Piped CLI input becomes a note in the notes folder like any other new
note; `~/.writ/piped/` is not kept as an exception, and the files already there migrate on the
same pass as scratch rows.

### 2. The notes folder

One folder, defaulting to `~/Writ`. The home folder root is not TCC-protected, so creating it on
first launch fires no permission prompt before the user has seen the app work. `~/Documents/Writ`
and iCloud Drive are one-click alternatives in Settings, where a prompt is expected because the
user asked for it.

Settings gains a section called Notes, showing the folder path with `~` collapsed, plus
`Show in Finder` and `Move`. The existing `Storage location` row, which shows a `writ.db` path
(`src/components/SettingsModal/SettingsModal.tsx:579`, indexed at `src/settings/index.ts:59`), is
never the answer to "where are my notes".

Files opened from elsewhere keep their path and are edited in place. Writ never creates a
subfolder and ships no `New folder` command; it lists, searches and opens notes inside subfolders
the user made in Finder.

The canonical notes-folder root becomes a blessed containment root in `AuthorizedPaths`.
`is_blessed_source` is exact set membership over canonical path strings
(`src-tauri/src/security/authorized_paths.rs:47-53`), blessed only at the three open sites
(`src-tauri/src/commands/file.rs:115`, `:155`, `:192`) and at startup from persisted buffer rows
(`src-tauri/src/state.rs:296-298`). A note that arrives in the folder from a sync client and is
opened from the sidebar therefore fails `authorize_source_write`
(`src-tauri/src/commands/file.rs:312-321`) with the message at
`src-tauri/src/commands/file.rs:17`. The fix is the `canonicalize_root` plus
`starts_with` containment shape already used for the workspace and inbox roots
(`src-tauri/src/security/authorized_paths.rs:79`, applied at `src-tauri/src/state.rs:182`
and `:193`).

`config.workspace.root` (`crates/writ-core/src/config/mod.rs:315-320`) becomes the folder the
user opened temporarily, not a second home.

A new note gets a file on the first keystroke, named by date, retitled once from its first line
under the first-run rules (only while the note has never been closed and no watcher event has
been observed for its path). No mirror survives for a note with no file. Three code sites assume
otherwise and are resolved in 0.4:

- `queries::list_scratch_candidates` selects on `source_path IS NULL`
  (`crates/writ-storage/src/database/queries.rs:221-230`, predicate at `:227`, placeholder title
  pattern at `:228`), and is the CLI and inbox reuse path.
- `read_source_if_diverged` returns `StorageError::Consistency` when `source_path` is `None`
  (`crates/writ-storage/src/buffer_store.rs:462-467`).
- The header comment of `crates/writ-storage/migrations/010_layout_state.sql:3-6` describes a
  scratch-acquires-a-path flow that no code implements and that this ADR finally makes real.

### 3. Notes are managed from inside Writ, and frontmatter survives

`New note` creates a file in the notes folder before anything else happens, visible in Finder
immediately. Renaming a tab renames the file through the shared sanitiser, and the tab keeps
its content, cursor and undo history; a rename that collides is refused with the colliding name
shown, and an empty or whitespace-only name is refused. A note is deleted to the Trash from the
sidebar, never unlinked. A note opened from elsewhere can be saved into the notes folder with
`Save a copy`, leaving the original untouched. The same operations are reachable from the CLI.
There is no `New folder` command (section 2).

A note that starts with a YAML frontmatter block opens, previews and saves without damage. The
preview hides the block or draws it as a compact list rather than rendering the leading `---` as
a horizontal rule, which is what `crates/writ-render` does today because it enables no
frontmatter handling. On save the block comes back byte-identical unless the user edited it, and
that holds for every path that rewrites a file for the user. A malformed or unterminated block is
body text, not swallowed.

### 4. Migration 040

`crates/writ-storage/migrations/040_notes_migration.sql` adds `buffers.migrated_path TEXT` and
`buffers.migrated_at INTEGER`, plus a `schema_meta` row recording that the notes migration ran.
Before the first write, `writ.db` is copied to `writ.db.pre-notes-migration` beside it and the
copy and its timestamp are recorded in `schema_meta`. The copy is kept for ten launches, then
deleted and its row cleared.

Order of operations on first launch of 0.4:

1. Resolve the notes folder. If the user has never chosen one, use the default and create it. No
   modal, no blocking picker.
2. Source-backed rows. Hash the mirror bytes and the source bytes and compare. On a match, set
   `migrated_path = source_path`, mark the row migrated, unlink the mirror. On a mismatch the
   mirror holds edits that never reached the file, which is the population produced by the save
   defect in 0.3.0 through 0.3.2: write the mirror bytes to
   `<notes>/Recovered/<name> (unsaved edits YYYY-MM-DD).md`, leave the source file byte-identical,
   record the row in the report, and only then unlink the mirror. A successful read of the source
   is never on its own a licence to delete a mirror. If the source is missing or unreadable, treat
   the row as scratch and fall through to step 3, writing into `<notes>/Recovered/`.
3. Scratch rows with content. Active rows are written to `<notes>/<title>.md` unattended. History
   rows are written to `~/.writ/archive/<title>.md` and stay there until the user acts on the
   report, because writing a hundred archived files into a folder that may sit in iCloud Drive,
   before the user has agreed to anything, makes the first act of the release an upload nobody
   asked for. Placeholder titles matching `writ-[0-9]*` and empty titles become the row's creation
   date in `YYYY-MM-DD` form. Collisions dedupe with ` 2`, ` 3`, matching Finder.
4. Verify then delete. For each row that produced a new file, compare the SHA-256 of the mirror
   bytes with the SHA-256 of the written file. Only on a match is `migrated_path` set and the
   mirror unlinked. On a mismatch, leave the mirror, record the row, and continue.
5. Report once, non-modal: how many notes are now files and where, plus the archive action when
   step 3 wrote any, plus a details link only when step 4 failed on some rows. The report never
   says a note is still inside Writ, because after 0.4 no read path returns mirror bytes.
6. Re-run is a no-op. Rows with `migrated_path` set and a file present at that path are skipped.

Empty rows (zero-length content, no source) are skipped and their rows deleted, so nobody
receives an archive folder of blank files.

Title sanitisation is one function in `writ-core`, shared by the migration, rename and
auto-title. It applies the strictest union of the three platforms on every platform, because the
migration runs unconditionally everywhere. It replaces control characters and `/ \ < > : " | ? *`
with a space;
strips leading dots, so a title never mints a hidden file; strips trailing dots and spaces, which
Windows silently drops and which would otherwise produce collisions the dedupe never sees;
suffixes the stem of the reserved device names `CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`,
`LPT1` to `LPT9` with `_`, with or without an extension (`NUL.md` becomes `NUL_.md`); collapses whitespace; and truncates to at most 120 grapheme
clusters and then to at most 200 UTF-8 bytes at a grapheme boundary, because APFS caps a filename
at 255 bytes rather than 255 characters.

The startup VACUUM added in 0.3.4 stays (`src-tauri/src/state.rs:156`,
`crates/writ-storage/src/maintenance.rs`). It runs before the window shows, which is the only
point at which the exclusive access it needs is available.

### 5. The write guard ships with the invariant, not with the conflict bar

Core holds `last_known_disk_hash` (SHA-256), size and mtime per source-backed note, set on open,
on reload, and after a successful save. A save is refused with a typed
`StorageError::SourceChangedOnDisk { path, disk_hash }`, added beside the existing `Consistency`
variant (`crates/writ-storage/src/errors.rs:44-48`), when the hash of the bytes on disk differs
from the last known hash and also differs from the content being saved. When the disk content is
byte-identical to what is being written, the save succeeds silently. mtime is never the conflict
signal: a touch, a sync round trip or a Time Machine restore changes it without changing content,
and a dialog that fires when nothing differs is worse than no check at all.

Reading is what materialises a placeholder, so every open entry point runs the dataless gate
before `classify_path` reaches the file (`dataless_open_answer` in
`src-tauri/src/commands/file.rs`, called from both `open_authorized_path` and
`open_confirmed_path`).

Every conflict resolution, not only "keep both", writes the losing side to disk as
`<name> (conflict YYYY-MM-DD HH.MM.SS).md` before it is applied. No code path can end a conflict
with zero files. Rename goes through the same guard, or a rename clobbers a file another process
created between the check and the move.

This comes forward into 0.4 deliberately. `save_to_source` and `write_source_and_mirror` write
unconditionally today (`crates/writ-storage/src/buffer_store.rs:397`, `:428`), and 0.4 removes the mirror's accidental
role as a second copy. Shipping one copy of the text behind an unconditional writer, with no
watcher and no history, would be worse than what 0.3.5 has.

The conflict bar is a later release. Until it exists, a refused save surfaces as a plain failure
saying the file changed on disk and the changes were not saved.

### 6. Ignore keys are namespaced before anything watches a source file

`IgnoreStamps` keys by a filename string with a content fingerprint and a five-second TTL
(`crates/writ-core/src/watcher/ignore.rs:30`, `:64`, `:80`). A source save records three keys: the
mirror filename, the full source path, and the source basename
(`src-tauri/src/commands/buffer.rs:125-128`, basename at `:128`). The basename is a global
namespace: in a notes folder holding `a/index.md` and `b/index.md`, a Writ save of one suppresses
a genuine external change to the other for up to five seconds, and it collides with the config
watcher, which keys on the bare name (`src-tauri/src/watcher/handler.rs:279-288`, against the
inbox watcher's full-path key at `:232-236` and the mirror watcher's bare-name key at `:304-329`).

Stamps become keyed by canonical absolute path, or by `(dev, ino)` where available. If a
bare-name key is still needed for the mirror space during the transition, it is namespaced as
`mirror:<filename>` against `source:<canonical path>`. Path comparison is canonical,
case-insensitive and Unicode-normalisation-insensitive: macOS delivers `/private/var/...` for
`/var/...` and NFD-normalised names, and Writ has shipped Arabic support since v0.3.2.
Canonicalisation resolves symlinks, so the stamp key and the watcher event path must be
canonicalised identically or every save looks like an external edit.

### 7. Search is re-keyed to paths

FTS moves off mirror text and onto canonical file paths under the notes folder. The index is
populated by a walk plus the watcher, and rebuilt on launch when absent, which is what makes
deleting `writ.db` safe. It never reads the content of a file the filesystem reports as not
downloaded, because reading an iCloud dataless file triggers materialisation by the provider
daemon.

Indexing does not block the UI. With a full reindex of 5,000 notes running, the first keystroke
is served within 50 ms at p95 and 150 ms at p99.

Quick open by note name is Cmd+Shift+O. Cmd+O keeps its Mac meaning of the Open dialog. The
retraining cost for switchers whose Cmd+O is a quick switcher is accepted deliberately.

### 8. Autosave cadence

Autosave for file-backed notes is 1000 ms idle, replacing the 300 ms default
(`crates/writ-core/src/config/mod.rs:60-62`), with a save on blur, on window hide, on tab close
and on quit. No note is written more than once per second, which bounds the write rate of a
paste-heavy or dictation session inside a sync folder.

One cadence covers notes and opened external files. Two cadences is a setting nobody can explain,
and the write guard makes the difference immaterial because a save that would clobber is refused
either way.

Quit has no mechanism today. Cmd+Q reaches `RunEvent::ExitRequested`
(`src-tauri/src/lib.rs:607`), which writes a shutdown snapshot from `collect_buffer_contents`
(`:628`), and that function reads files off disk (`crates/writ-storage/src/buffer_store.rs:703`) rather than asking the
frontend for the live document. Text typed inside the last debounce window is therefore in
neither the file nor the snapshot, and after this change the same function reads source files
with the same gap. 0.4 closes it: quit is intercepted on the Rust side long enough to request and
await a frontend flush, or the snapshot is fed by the editor.

The 30-second snapshot heartbeat (`src-tauri/src/lib.rs:553-554`) is slowed, and a database
bloat guard test lands with it.

### 9. Startup guards

Writ refuses to start, with a plain-language error naming the folder, when its data directory
resolves inside a sync provider's tree. WAL over a sync provider is documented-unsafe. The paths
checked are, on macOS, `~/Library/Mobile Documents`, `~/Library/CloudStorage/`, `~/Dropbox` and
`~/Google Drive`; on Windows, `%USERPROFILE%\OneDrive`, `%USERPROFILE%\Dropbox` and
`%USERPROFILE%\Google Drive`; on Linux, `~/Dropbox`, `~/Google Drive`, and any directory
containing a `.stfolder` marker. `resolve_writ_dir` (`src-tauri/src/state.rs:310`) is where the
resolution happens.

The database is never placed inside the notes folder, asserted at startup on all three platforms.

`F_FULLFSYNC` is not adopted here. Its p99 cost is measured on an internal APFS volume, an
external USB volume and a network mount at the one-second cadence; the measurement is the
deliverable, and adoption is a later decision.

### 10. The banned-word test

A test asserts that a fixed word list appears in no string under `src/**` that reaches the DOM,
and in no Rust string returned to the frontend as a user-facing message. The list is `vault`,
`buffer`, `scratchpad`, `second brain`, `render surface`, `inbox`, `reveal`, `threshold`, `refuse`, `debounce`,
`source`, `dialect`, `FTS`, `IPC`, `sidecar`, `MiB`, `syntax highlighting`, `typography`.
`src-tauri/tests/cli_symlink_tests.rs` is the existing precedent for a guard test of this shape.

The scope is user-visible strings only. Type names, field names and comments stay legal, and a
test that reads whole files rather than rendered strings would fail on this document.

The test lands in 0.4 so that every string written for the conflict bar and the save states is
born compliant. The row renames themselves come later, so live strings such as
`src/components/Sidebar/SearchBar.tsx:46` and `src/components/Preview/PreviewLayout.tsx:133` are
fixed by the same release the test lands in, and the settings rows follow.

### 11. The index schema is created now

Migration 040 creates these tables empty, so the link and index work that follows does not need a
050:

```
files      (path PRIMARY KEY, size, mtime, hash, indexed_at)
links      (from_path, to_target, to_path, kind, line, col)
properties (path, key, value_json)
tags       (path, tag, line)
headings   (path, level, text, line, slug)
```

All four dependent tables are keyed by canonical path with `ON DELETE CASCADE` from `files`, so
a note that leaves the folder takes its derived rows with it. The rationale is that the graph,
backlinks, the MCP tools and the CLI `writ links` are all queries over these tables, and creating
them alongside the path re-key costs one migration instead of two against a schema that has
already been rewritten once.

### 12. What ADR-020 and ADR-024 mean now

ADR-020 stands. The deferred, coalesced reindex, the `save_content_without_index` and
`reindex_buffer` pair, the per-buffer generation counter and the shutdown flush all keep their
contracts. What changes is the key and the source of the indexed bytes: a reindex reads the
canonical file under the notes folder instead of a mirror. The ADR-020 argument that reading from
disk rather than from a captured string makes coalescing safe holds either way.

ADR-024 stands. Arrival detection by path-set membership rather than timestamp is unaffected by
where note text lives. What changes is what an arrival becomes: a file that lands in the watched
folder is opened as the file it already is, not copied into a mirror. The arrival snapshot keeps
only the pre-existing path set for the lifetime of the watch, which holds no text. The watched
folder itself moves under Advanced and stops being a sidebar section.

## Consequences

**Positive**

- Deleting `writ.db` is safe. It costs a reindex and the session layout, and loses no note.
- Finder shows real files with human names, which is what the product promises.
- Sync works by putting the notes folder in a sync client. Writ ships no sync service and needs
  none.
- The privacy page gets simpler and truthful: the database holds an index, history metadata and
  window state, and the text is in the files.
- Three copies of the text collapse to one, which removes the class of defect where the copies
  disagree about what the user wrote.

**Negative and risks**

- Search is only as fresh as the watcher. A note changed by another program is stale in the index
  until the watcher event arrives and the debounce window closes.
- The compare read that the write guard performs materialises an iCloud dataless file unless the
  download status is checked first. The guard must check before it reads.
- A TOCTOU window remains between the compare and the atomic rename
  (`crates/writ-storage/src/atomic.rs:60`, temp file created in the destination directory at
  `:68`). It cannot be closed without a lock. Watching the folder keeps it normally closed by
  having already delivered the change.
- The migration runs unconditionally against real databases; the one measured held 114 rows
  and 37 MB of mirrors. Verify-then-delete and the rollback copy bound the
  damage; they do not undo a run that mis-titled or mis-deduped the files.
- Public claims become false the day 0.4 ships. `README.md:56` and `:139`,
  `site/src/pages/privacy.astro:21-24` and `:30`, and the site's remaining `buffer` strings describe
  storage that no longer exists. Per the locked plan these are updated with the 1.0 release, not
  before, so 0.4 ships with a repo and a site that describe the old model.

**Follow-on records**

- ADR-029 records the 2026-08-25 direction: links, graph, an AI-readable index, an MCP harness,
  a plugin layer, platforms and exclusions, and the vocabulary.
- ADR-030 records the design system: DTCG token source, generated outputs, the platform layer,
  the accent, fonts and icons.
- ADR-031 records the AI harness and its threat model, later.
- ADR-032 records the plugin API, later.
