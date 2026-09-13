# Captures for the site

Every still under `site/src/assets/captures/` comes from `scripts/capture/run.sh`: a release build of the current branch, launched on a scratch copy of `scripts/capture/fixtures/home/Notes` with `WRIT_DATA_DIR` and `WRIT_NOTES_DIR` pointed at it, driven by keystrokes posted to that one process, and captured by its window id at 2x. Nothing in a capture identifies the machine, and the run never reads `~/.writ` or `~/Writ`.

## Re-run

```sh
scripts/capture/run.sh --all                                  # every scene, light and dark
scripts/capture/run.sh --scene search --scene chat            # a few scenes
scripts/capture/run.sh --scene hero-window --theme light      # one theme
scripts/capture/run.sh --scene hero-window --shell win        # the Windows shell, from a dev instance
scripts/capture/run.sh --all --no-build                       # reuse the last bundle
```

The run builds the CLI sidecar and the app (`CARGO_PROFILE_RELEASE_STRIP=false`, no updater artifact) and logs the bundle path. It refuses to start while any other Writ process exists, and every key and click waits until the instance is the frontmost app and the machine has been idle for 45 s, so leave it alone once it is going. The Finder still and the first-run still follow the system appearance; the run flips it for the dark take and puts it back.

Output: `site/src/assets/captures/<scene>-<theme>.png`, at most 2880 px wide and 1.2 MB each (`shrink.mjs`), plus a contact sheet at `.status/v2/shots/captures-contact.png`. `Capture.astro` picks a still up by name, so a page references `<Capture name="search" />` and the theme pair follows.

## Scenes

| Name | On screen | Window |
|---|---|---|
| `hero-window` | Sidebar with the folder, tags folded, a meeting note, Connections open | 1440x900 |
| `hero-window-win`, `hero-window-linux` | The same frame in the Windows and GNOME shells (`VITE_WRIT_PLATFORM`, dev instance) | 1440x900 |
| `notes-folder` | Finder, list view, on the notes folder: one `.md` per note, folders as folders | 1280x800 |
| `connections` | The Connections panel beside a note: the notes that link here with their sentences, the outline, the properties | 1280x800 |
| `graph-folder` | The Graph view of the whole folder, one colour per folder, one note found from the search box | 1280x800 |
| `graph-local` | Nearby notes in the Connections panel | 1280x800 |
| `search` | Search everywhere with matches across several notes | 1280x800 |
| `preview-rich` | Source beside the rendered note: a callout, a table, a Mermaid diagram, display math | 1280x800 |
| `chat` | The chat pane beside a note with a proposed change shown next to the text; the host is a stub on localhost | 1280x800 |
| `versions` | Earlier versions of a note, the Revert To… panel | 1280x800 |
| `activity` | The Activity panel: what an approved program did with the notes, no note text | 1280x800 |
| `settings-appearance` | Settings, Appearance: light and dark, the six accents, interface text size | 1280x800 |
| `obsidian-folder` | A folder written in Obsidian, open as it is: links resolved, aliases and properties listed | 1280x800 |
| `today` | First run on an empty folder: today's note and the hint line | 1280x800 |
| `tags` | The sidebar with the nested tags open | 1280x800 |

## The fixture

Sixteen notes a person might keep: a trip, a reading list, two recipes, three weekly reviews, a small garden project whose notes link both ways, a meeting note, a sourdough note carrying the callout, table, diagram and math, a newsletter draft, and a folder written in Obsidian with `aliases:` and a property block. Names and text are invented. Nothing on screen says buffer, vault, scratchpad or second brain. `fixtures/versions/` holds the earlier texts the versions scene seeds.

## Recordings

Deferred to the announcement week: the hero loop (`summon`), the find band, the inbox and themes loops, and the Obsidian side-by-side. `Loop.astro` already plays `site/public/media/<name>-{light,dark}.{webm,mp4}` on scroll with the still as the poster, so a loop lands by dropping the files in. Encode targets from the design standard: hero mp4 under 1.2 MB, webm under 0.8 MB, poster under 60 KB. They come from the same harness with a recording step, not from a hand-held take.
