# Opening a folder written in Obsidian

There is no import step. Point Writ at the folder Obsidian was using and the
notes open from where they are: `Settings → Notes → Notes folder`, or run Writ
with `WRIT_NOTES_DIR` set to the folder. Nothing is copied, converted or
rewritten, and both apps can keep using the same files.

Writ reads the folder once on the first launch after it is set, then follows
changes on disk. Links, properties, tags and headings come out of that read.

## What carries over

| Written in Obsidian | In Writ |
|---|---|
| Folders, file names, plain markdown | The files are the notes. Nothing moves. |
| `[[Note]]`, `[[Folder/Note]]` | Opens the note. A folder in front of the name picks between notes that share it. |
| `[[Note\|a label]]` | Opens the note, and the label is what the backlink list shows. |
| `[[Note#Heading]]` | Opens the note at that heading. |
| A name two notes answer to | Both notes are offered. Writ never picks one for you. |
| Links pointing at a note | The backlink list, with the sentence each link sits in. |
| Frontmatter `key: value` and `key: [a, b]` | Properties, listed with the note. |
| A nested frontmatter map | Kept exactly as written and shown as text. |
| `#tag`, `#project/alpha` | Tags. `#project/alpha` is one tag, listed under `project`. |
| Frontmatter `tags:` and `tag:`, as a list, as items, or as one line of comma- or space-separated names | Tags, same as the ones written in the text. |
| `#Project` in one note, `#project` in another | One tag. The list files tags lowercased. |
| A `#tag` inside a code fence | Left alone. It is an example, not a tag. |
| Headings | The outline beside the note, and the anchors links point at. |
| `> [!note]`, `> [!warning]`, and the other ten types | Callouts, aliases included. One written closed shows its title only. |
| `![[Note]]` | The note is shown in the page, up to three embeds deep. |
| `![[image.png]]` | The image is shown. |
| ` ```mermaid ` fences, `$x^2$`, tables, task lists, footnotes | All rendered, offline. Nothing is fetched from the web. |

## What does not

| Written in Obsidian | What to do |
|---|---|
| `.obsidian/` settings: appearance, hotkeys, core plugin options | Set them again in Writ's settings. The folder is skipped whole and never written to, so Obsidian keeps working from it. |
| Community plugins in `.obsidian/plugins/` | None of them run. A note a plugin wrote shows the text the file holds, so a Dataview query or a Templater tag renders as the characters it is. |
| `.canvas` files | Not drawn. The file opens as the text it holds. Keep the drawing in Obsidian. |
| `obsidian://` links | Not opened. Rewrite the ones worth keeping as `[[Note]]`. |
| `aliases:` in frontmatter | Kept as a property, but a link that names an alias finds nothing. Link by file name, or rename the note. |
| Block references, `[[Note#^a1b2]]` | The link opens the note at its top. |
| Embedding one section, `![[Note#Heading]]` | The whole note is shown. |
| `.trash/` | Skipped, so a note deleted in Obsidian stays out of the way. To bring one back, move the file out of `.trash/`. |

## What Writ leaves behind

Its own settings and index live in the app's data folder, not beside the notes.
The only thing Writ writes into the notes folder is the notes: files you create,
files you edit, and nothing else.

Implementation: `crates/writ-storage/src/notes_index.rs`, with the folder this
page describes as a fixture in
`crates/writ-storage/tests/fixtures/obsidian-folder/`.
