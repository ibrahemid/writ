# `writ` verbs

The `writ` command opens files (ADR-017) and answers questions about the Writ
folder. This page is the record of the second half: the verbs, their output, and
the exit codes a script can rely on.

Implementation: `crates/writ-cli/src/verbs.rs` and
`crates/writ-cli/src/host_verbs.rs`; the command line is read by clap in
`crates/writ-cli/src/args.rs`, so `writ --help` lists every verb and
`writ <verb> --help` prints its arguments.

## Where the answers come from

Each verb resolves the folder from the same three sources in the same order the
app uses: `WRIT_NOTES_DIR`, then `[notes] root` in `config.toml`, then
`<WRIT_DATA_DIR>/Writ` or `<home>/Writ`. It reads the index in
`<WRIT_DATA_DIR or ~/.writ>/writ.db`.

A source that names a relative folder is skipped and the next one is tried,
which is what the app does with the same config. The command creates nothing,
so a configured folder that is not there is still the folder named, and the
verb that needs it says it could not be read.

The index is opened read-only. The command runs no migration, writes no row and
creates no database. Reading a WAL database does make SQLite create the `-shm`
and `-wal` companions when they are absent; no frame is written into either.

Link resolution is `writ_core::notes::links` reached through `writ-storage`, the
same policy the editor, the preview and the index itself use. Nothing in the CLI
re-implements it.

`open` and `today` open a window; no other verb does. `writ <path>` opens a
file too.

`search`, `cat`, `ls`, `append`, `today` and `open` run on
`writ_storage::note_host::NoteHostImpl`, the surface `writ mcp` serves from
(ADR-032), so the command line and a connected program read and write a file
the same way. A file outside the notes folder is refused by these verbs with
exit 1, as it is for a connected program.

## The verbs

| Command | Answers |
|---|---|
| `writ links <file> [--json]` | every link written in the file |
| `writ backlinks <file> [--json]` | every link in another file that points at it |
| `writ properties <file> [--json]` | the file's frontmatter properties |
| `writ tags <file> [--json]` | the file's tags, with the line each is on |
| `writ tags [--json]` | every tag in the folder, with a file count |
| `writ search <query> [--limit N] [--json]` | files whose text matches, best match first (20 unless `--limit`) |
| `writ cat <file> [--json]` | the file's text, as written |
| `writ ls [<folder>] [--json]` | every file in the folder, or in one folder inside it |
| `writ append <file> [--json]` | adds piped stdin to the end of the file |
| `writ today [--append] [--no-open] [--json]` | opens today's file, creating it when the day has none |
| `writ open <name>[:<line>] [--wait] [--json]` | opens a file in the folder by name, at a line |
| `writ new [<name>] [--json]` | creates a file, holding piped stdin when there is any, and prints its path |
| `writ rename <file> <new-name> [--json]` | renames a file inside its folder |
| `writ trash <file> [--json]` | moves a file to the trash and prints where it was |
| `writ mcp` | serves the folder to an MCP client over stdin and stdout |

A `<file>` is a path, absolute or relative to the current directory, or the
name of a file in the folder, with or without its extension. A name is looked
for as written, then with the configured extension (`[files] default_extension`),
then with `.md`. A read verb that finds none of those asks the index, and a
name two files answer to there is refused with both paths rather than resolved
to one.

A first argument that is one of the verb names runs the verb. A file of that
name is opened by spelling a path for it: `writ ./links`.

## Opening, lines and `--wait`

`writ <path>`, `writ open` and `writ today` open the file in Writ. A path or a
name may end in `:<line>` or `:<line>:<column>` to open at that line:
`writ Plan.md:12`. The suffix is read as a line only when no file carries the
whole argument as its name, so a file called `log:12` opens as written.

`--wait` returns once every file it opened has been closed in Writ, which is
what an editor command has to do:

```
git config --global core.editor "writ --wait"
```

Closing the tab ends the wait with exit 0, and so does quitting Writ. A folder
cannot be waited on (exit 2). A plain open goes through the operating system's
open-files path. An open carrying a line or a wait is written into
`<data dir>/open-requests/` for the running Writ to take, starting it first
when it is not running; when nothing takes the request within 10 seconds (60
when the command had to start Writ), it is withdrawn and the command exits 1.
ADR-017 records the channel.

`--no-open` on piped text saves it and prints its path without opening Writ:
`make 2>&1 | writ --no-open --title "build log"`. Given with a file path it is
exit 2. `today --no-open` prints the day's path without opening it.

`writ mcp` is started by the client, opens no port, and answers no tool call
until Connected programs is on in Settings, Apps and the client is approved
there. `writ mcp --help` prints its usage.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | the verb ran |
| 1 | the file, the index or the operation was not there |
| 2 | the arguments could not be read |

Code 1 covers: no file of that name, a name two files answer to, a file the
index does not hold yet, an absent index, an index written under a different
schema version than this build reads, a file outside the notes folder, a file
over 2 MB for `cat` and `append`, a failed create, rename, trash or append, and
an open Writ did not take. Code 2 prints what was wrong and the usage on
stderr. `append` and `today --append` with nothing piped exit 2. The
file-opening path uses the same codes: two folders at once, a file and a folder
together, `--wait` on a folder, `--no-open` with a path, and `writ -` with
nothing piped exit 2.

## Default output

One record per line on stdout, tab-separated, in the field order below. A tab or
a line break inside a field is replaced with a space so a record stays on one
line; `--json` carries the field as written. No header line, so the output pipes
into `cut` and `awk` directly.

| Verb | Fields |
|---|---|
| `links` | `line`, `col`, `kind`, `status`, `target`, `path` |
| `backlinks` | `from_path`, `from_name`, `kind`, `line`, `col`, `certainty`, `target`, `alias`, `context` |
| `properties` | `key`, `value` |
| `tags <file>` | `tag`, `line` |
| `tags` | `tag`, `notes` |
| `search` | `path`, `line`, `excerpt` |
| `ls` | `path`, `name`, `bytes` |
| `new`, `rename`, `trash`, `append`, `today`, `open` | the file's path, alone on the line |

`cat` prints the file's text and nothing else.

For `links`, `path` holds the resolved file's path when `status` is `resolved`,
the candidate paths joined by `, ` when it is `ambiguous`, and nothing when it is
`unresolved`. A path containing `, ` is why the exact list is in `--json`.

Tags are printed as the index stores them, without the leading `#`.

For `properties`, `value` is printed as the JSON below, quotes and all, so a
string is told apart from a number and a multi-line value stays on one record.

### The line a name-only file earns

A file whose bytes are not on this machine is indexed by its name alone, so its
links, properties and tags are empty because nothing was read, not because there
are none. `links`, `properties` and `tags <file>` say so on stderr and still exit
0:

```
writ: this file has no data on this machine, so nothing was read out of it
```

Under `--json` the line is not printed; the document's `indexed_by` field carries
it instead. `backlinks` never prints it: a file held by name alone still has
backlinks, because they are written in other files.

## `--json`

One JSON document per invocation, on one line, on stdout. Keys are snake_case.
Every list is a JSON array, never an object. A field with no value is `null`,
never an empty string.

The four read verbs carry `note` (the file's path as the index keys it) and
`indexed_by` (`"content"` or `"name"`).

### `writ links <file> --json`

```json
{
  "note": "/Users/x/Writ/One.md",
  "indexed_by": "content",
  "links": [
    {"target": "Two", "kind": "wikilink", "line": 10, "col": 4,
     "status": "resolved", "path": "/Users/x/Writ/Two.md", "candidates": []},
    {"target": "Ghost", "kind": "wikilink", "line": 10, "col": 13,
     "status": "unresolved", "path": null, "candidates": []},
    {"target": "Dup", "kind": "markdown", "line": 11, "col": 0,
     "status": "ambiguous", "path": null,
     "candidates": ["/Users/x/Writ/a/Dup.md", "/Users/x/Writ/b/Dup.md"]}
  ]
}
```

`kind` is `wikilink` or `markdown`. `status` is `resolved`, `ambiguous` or
`unresolved`. `path` is set only when `status` is `resolved`; `candidates` is
non-empty only when it is `ambiguous`. `line` is 1-based, `col` is a 0-based
character offset in that line.

### `writ backlinks <file> --json`

```json
{
  "note": "/Users/x/Writ/One.md",
  "indexed_by": "content",
  "backlinks": [
    {"from_path": "/Users/x/Writ/Two.md", "from_name": "Two",
     "kind": "wikilink", "line": 1, "col": 8, "certainty": "resolved",
     "target": "One", "alias": "the first", "context": "Back to [[One|the first]]."}
  ]
}
```

`certainty` is `resolved` or `ambiguous`, the same two words the app uses. A
link that resolved to a different file of the same name, and a link that
resolved to nothing, are backlinks of no file and appear in no list.

`alias` is a wikilink's `|alias` and is `null` for every markdown link, whose
label the parser does not keep. `context` is the sentence the link sits in, cut
from the text the index holds; it is empty when the index holds no text for the
linking file.

### `writ properties <file> --json`

```json
{"note": "/Users/x/Writ/One.md", "indexed_by": "content",
 "properties": [{"key": "title", "value": "One"}, {"key": "tags", "value": ["a", "b"]}]}
```

`value` is the property's value as JSON: a string, a number, a boolean, `null`
or an array, matching what the frontmatter held. A nested mapping is not reduced
to a JSON object; it arrives as the string of the block as written, line breaks
included. Properties are listed in the order they were written.

### `writ tags <file> --json`

```json
{"note": "/Users/x/Writ/One.md", "indexed_by": "content",
 "tags": [{"tag": "idea", "line": 8}, {"tag": "draft", "line": 8}]}
```

### `writ tags --json`

```json
{"note": null, "notes_folder": "/Users/x/Writ",
 "tags": [{"tag": "idea", "notes": 12}, {"tag": "draft", "notes": 3}]}
```

`note` is `null`, which is what tells the two `tags` documents apart. Tags are
ordered by file count descending, then by tag. `notes` counts files, not
mentions: a file tagged twice with one tag counts once.

`notes_folder` is the folder this invocation resolved, from `WRIT_NOTES_DIR`,
then the config, then the default. The tags come from the index, which
describes the folder the app last read. They are the same folder unless
`WRIT_NOTES_DIR` points somewhere the app has never indexed, so read the field
as where this command was pointed rather than as where a tag was found.

### `writ new --json`, `writ trash --json`

```json
{"note": "/Users/x/Writ/Ideas.md"}
```

### `writ search --json`

```json
{"query": "launch",
 "hits": [{"path": "/Users/x/Writ/Plan.txt", "name": "Plan.txt", "line": 1,
           "excerpt": "launch the site"}]}
```

`line` is 1-based and `null` when the match is in the file's name only.

### `writ cat --json`

```json
{"note": "/Users/x/Writ/Plan.txt", "bytes": 29,
 "hash": "<sha-256 of the file's bytes>", "text": "launch the site\n..."}
```

`text` is decoded from the file's own encoding (ADR-049); `hash` is of the
bytes on disk.

### `writ ls --json`

```json
{"notes_folder": "/Users/x/Writ",
 "notes": [{"path": "/Users/x/Writ/Plan.txt", "name": "Plan.txt", "bytes": 29}]}
```

A `<folder>` is matched against each path relative to the notes folder, so
`writ ls projects/` lists what is inside `projects`.

### `writ append --json`

```json
{"note": "/Users/x/Writ/Plan.txt", "bytes": 32,
 "hash": "<sha-256 of the file's bytes>", "changed": true}
```

`changed` is `false` when the piped text held nothing but blank space and
nothing was written.

### `writ today --json`

```json
{"note": "/Users/x/Writ/2026-10-15.txt", "created": false, "changed": true}
```

`created` is whether this run made the day's file; `changed` whether
`--append` added anything.

### `writ open --json`

```json
{"note": "/Users/x/Writ/Plan.txt", "line": 12}
```

`line` is `null` when the name carried none. The document is printed once
Writ has taken the file.

### `writ rename --json`

```json
{"note": "/Users/x/Writ/Renamed.md", "previous_path": "/Users/x/Writ/Ideas.md"}
```

## What the writing verbs do and do not do

`append` reads piped stdin, takes off blank lines before the text and blank
space after it, and adds the rest to the end of the file on lines of its own,
in the line ending most of the file's lines use and in the file's encoding
(`writ_core::capture`, the rule quick capture follows). A character the
encoding has no bytes for refuses the append with exit 1. The write goes
through the guard against the bytes the append read and is tried again when
the file changed in between; before it lands, the app's version store keeps
what the file held, so Revert To in Writ can put it back. A file outside the
app's notes folder keeps no version, as in the app. A bare name no file has yet
is created in the notes folder holding the text. `today --append` does the same
to the day's file, `<yyyy-mm-dd>.<ext>`, found and minted by the rule the
window's Today's File uses.

`new` with piped stdin creates the file holding the text as piped.

`new` with no name mints the name the window's New File mints,
`writ-<yymmdd>-<hhmm>` in local time (`writ_core::notes::minted_stem`). A name
you give goes through the app's sanitiser (`writ_core::notes::note_file_stem`),
and one that sanitises to nothing is named for today's date. Either way the name
is deduped against the folder with a hyphenated counter, so a second file in the
same minute is `writ-260921-0748-2` (`dedupe_file_name`). A name ending in
`.md`, `.markdown`, `.txt` or `.text` sets the extension, so `writ new
Groceries.md` makes a Markdown file whatever the config says
(`writ_core::notes::explicit_extension`); otherwise the extension is the
configured one, `[files] default_extension`, `txt` or `md`. The file is created
empty. `new` needs no index.

`rename` and `trash` go through `writ_storage::note_ops`, the same functions the
app's menu items call. Neither passes a stamp: the stamp exists to keep the app
from reading its own write back as somebody else's, and from another process
there is nothing to suppress. The app's watcher sees a rename from the command
line as the outside change it is.

Neither updates the index. The app picks the change up through its watcher while
it is running, and through its next pass over the folder otherwise, so a read
verb run in between can name a file that has moved.

`rename` does not rewrite links that name the file by its old name. The usage
text says so too, so nobody learns it from a broken link. Rewriting them is the
app's, and the command line gains the option once the app has it.

The new name goes through `writ_core::notes::rename_stem`, the function the
app's rename runs. The file's own extension comes off a name that ends in it, so
`rename Draft "Foo.md"` on `Draft.md` gives `Foo.md` and not `Foo.md.md`. A new
name ending in `.md`, `.markdown`, `.txt` or `.text` sets the file's format, so
`rename Draft "Foo.md"` on `Draft.txt` gives `Foo.md`; any other name keeps the
file's extension. The characters no filename may carry on any platform
(`/ \ < > : " | ? *` and control characters) become spaces, on every platform,
so a name minted here is one the app would mint and one that survives a sync
onto another machine. What comes back is a name and never a path: an absolute
new name or one carrying `..` loses the separators that would take the file out
of its folder, and a name that survives to nothing is refused with
`That name is empty.` and exit 1.
