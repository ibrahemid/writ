# Opening an Obsidian folder in Writ

There is no import step. Point Writ at the folder Obsidian was using and the
files open from where they are: Settings, Files, Folder, or run Writ with
`WRIT_NOTES_DIR` set to the folder. Nothing is copied, converted or rewritten,
and both apps can keep using the same files.

Writ reads the folder once after it is set, then follows changes on disk.
Links, properties, tags and headings come out of that read.

Backlinks, properties and the outline are in the Connections panel, tags are in
the sidebar, and the graph is its own view. Each is an app, off on a fresh
install: switch on Connections, Tags and Graph under Settings, Apps.

## What carries over

| Written in Obsidian | In Writ |
|---|---|
| Folders, file names, plain Markdown | The files stay where they are. |
| `[[Note]]`, `[[Folder/Note]]` | Opens the file. A folder in front of the name picks between files that share it. |
| `[[Note\|a label]]` | Opens the file, and the label is what the Connections list shows. |
| `[[Note#Heading]]` | Opens the file at that heading. |
| A name two files answer to | Both files are offered. Writ never picks one for you. |
| Links pointing at a file | Connections lists them, with the sentence each link sits in. |
| Frontmatter `key: value` and `key: [a, b]` | Properties, listed in Connections. |
| A nested frontmatter map | Kept exactly as written and shown as text. |
| `#tag`, `#project/alpha` | Tags. `#project/alpha` is one tag, listed under `project`. |
| Frontmatter `tags:` and `tag:`, as a list, as items, or as one line of comma- or space-separated names | Tags, same as the ones written in the text. |
| `#Project` in one file, `#project` in another | One tag. Tags are filed lowercased. |
| A `#tag` inside a code fence | Left alone. It is an example, not a tag. |
| `#fff` and `#0a7d4f` in pasted CSS, `href="#top"` and `url(#arrow)` in pasted HTML | Left alone. A colour and an anchor are not tags. |
| Headings | Heading sizes in Inline mode, the outline in Connections, and the anchors links point at. |
| `![alt](image.png)` | The image is shown under its line. |
| Task lists | Checkboxes you can click. |

## What does not

| Written in Obsidian | In Writ |
|---|---|
| `.obsidian/` settings: appearance, hotkeys, core plugin options | Set them again in Writ's settings. The folder is skipped whole and never written to, so Obsidian keeps working from it. |
| Community plugins in `.obsidian/plugins/` | None of them run. A file a plugin wrote shows the text the file holds, so a Dataview query or a Templater tag shows as the characters it is. |
| `> [!note]` callouts | Styled as a blockquote. |
| `![[Note]]` and `![[image.png]]` embeds | Not shown in the page. The text stays as written. |
| ` ```mermaid ` fences | A code block, not a diagram. |
| `$x^2$` math, footnotes | Shown as written. |
| `.canvas` files | Not drawn. The file opens as the text it holds. Keep the drawing in Obsidian. |
| `obsidian://` links | Not opened. Rewrite the ones worth keeping as `[[Note]]`. |
| `aliases:` in frontmatter | Kept as a property, but a link that names an alias finds nothing. Link by file name, or rename the file. |
| Block references, `[[Note#^a1b2]]` | The link opens the file at its top. |
| `.trash/` | Skipped, so a file deleted in Obsidian stays out of the way. To bring one back, move it out of `.trash/`. |

## What Writ keeps outside the folder

Settings, the index and earlier versions of each file live in Writ's data
folder (`~/.writ`), not beside your files. Inside the folder Writ writes the
files you create and edit, and a dated conflict copy beside a file when a save
would overwrite a newer version on disk.

Implementation: `crates/writ-storage/src/notes_index.rs`, with the folder this
page describes as a fixture in
`crates/writ-storage/tests/fixtures/obsidian-folder/`.
