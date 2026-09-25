# Captures for the site

Every still under `site/src/assets/captures/` comes from `scripts/capture/run.sh`: a release build of the current branch, launched on a scratch copy of the folder `scripts/capture/fixtures/home/Notes` with `WRIT_DATA_DIR` and `WRIT_NOTES_DIR` pointed at it, driven by keystrokes posted to that one process, and captured by its window id at 2x. Nothing in a capture identifies the machine, and the run never reads `~/.writ` or `~/Writ`.

## Re-run

```sh
scripts/capture/run.sh --all                                  # every scene, light and dark
scripts/capture/run.sh --scene search --scene chat            # a few scenes
scripts/capture/run.sh --scene hero-window --theme light      # one theme
scripts/capture/run.sh --scene hero-window --shell win        # the Windows shell, from a dev instance
scripts/capture/run.sh --all --no-build                       # reuse the last bundle
scripts/capture/run.sh --scene settings-programs              # report stills, into .status
scripts/capture/run.sh --scene loop-markdown                  # one loop, light and dark
scripts/capture/run.sh --report                               # media sizes against their limits
```

Each scene writes its own `config.toml`: `[files] default_extension = "txt"`, Markdown files in Inline mode, the pine accent, and only the apps that scene shows switched on (`apps_on` in `run.sh`), so `[apps]`, `ai.chat.enabled`, `ai.rewrite.enabled` and `mcp.enabled` are all written out. `first-run` writes no config, which is what puts the format step on screen.

The run builds the CLI sidecar and the app (`CARGO_PROFILE_RELEASE_STRIP=false`, no updater artifact) and logs the bundle path. It refuses to start while any other Writ process exists, and every key and click waits until the instance is the frontmost app and the machine has been idle for 45 s, so leave it alone once it is going. The Finder still and the `first-run` still follow the system appearance; the run flips it for the dark take and puts it back.

Stills are 2x. When the main display draws at 1x, as the headless M1's fallback display does, the run adds a 1680x1050 display drawn at 2x (`drive hidpi`, through `CGVirtualDisplay`) for its own length and places every window on it; on a machine with no display attached, that display stands in for the fallback one until the run ends, so a remote view of the machine changes size meanwhile. `CAPTURE_HIDPI=0` keeps the display as it is and takes 1x stills.

Output: `site/src/assets/captures/<scene>-<theme>.png`, at most 2880 px wide and 1.2 MB each (`shrink.mjs`), plus a contact sheet at `.status/v2/shots/captures-contact.png`. `Capture.astro` picks a still up by name, so a page references `<Capture name="search" />` and the theme pair follows.

## Scenes

| Name | On screen | Apps on | Window |
|---|---|---|---|
| `hero-window` | The file tree and search in the sidebar, the garden committee file open in Inline mode with the cursor on a list line, so that line shows its markup | none | 1440x900 |
| `hero-window-win`, `hero-window-linux` | The same frame in the Windows and GNOME shells (`VITE_WRIT_PLATFORM`, dev instance) | none | 1440x900 |
| `text-file` | `To do.txt` in the plain editor, with the status bar a `.txt` file gets | none | 1280x800 |
| `markdown-inline` | `Sourdough notes.md` in Inline mode: a callout, a table shown as styled source, checkboxes and a code block | none | 1280x800 |
| `search` | Search everywhere with matches across `.txt` and `.md` files | none | 1280x800 |
| `apps` | Settings, Apps, with Graph on and the other five off | Graph | 1280x800 |
| `first-run` | No config and an empty folder: the format step, Plain text or Markdown | none written | 1280x800 |
| `notes-folder` | Finder, list view, on the folder: `.md` and `.txt` files, folders as folders | none | 1280x800 |
| `connections` | The Connections panel beside a file: the files that link here with their sentences, the outline, the properties | Connections | 1280x800 |
| `graph-folder` | The Graph view of the whole folder, a colour per top-level folder, the search field focused and empty: the first and last frame of `loop-graph` | Graph | 1280x800 |
| `graph-local` | Nearby files in the Connections panel | Connections, Graph | 1280x800 |
| `preview-rich` | `Sourdough notes.md` in Inline mode with the sidebar closed | none | 1280x800 |
| `chat` | The chat pane beside Birthday ideas with the file attached, Sourdough notes attached by `@`, the rendered reply and the proposed edit as a diff; the host is a stub on localhost | Chat | 1280x800 |
| `versions` | Earlier versions of a file, the Revert To… panel | none | 1280x800 |
| `activity` | The Activity panel: what an approved program did with the files, no file text | Connected programs | 1280x800 |
| `settings-appearance` | Settings, Appearance: light and dark, the six accents, interface text size | none | 1280x800 |
| `obsidian-folder` | A folder written in Obsidian, open as it is: links resolved, aliases and properties listed | Connections | 1280x800 |
| `tags` | The sidebar with the nested tags open | Tags | 1280x800 |

`settings-programs` is a report scene: the Connected programs rows at three interface text sizes and at the narrowest window, written to `.status/reports/programs-section/`. It runs only when named.

## The fixture

Nineteen files a person might keep. Three are plain text: a to-do list, a camping kit list and a server log excerpt. Sixteen are Markdown: a trip, a reading list, two recipes, three weekly reviews, a small garden project whose files link both ways, meeting minutes, a sourdough file carrying the callout, table, checkboxes, code block, diagram and math, a newsletter draft, and a folder written in Obsidian with `aliases:` and a property block. Names and text are invented, and the log uses documentation addresses. `fixtures/versions/` holds the earlier texts the versions scene seeds.

## Recordings

`chat` and the six loop scenes record the window while the app is driven, once per theme, each take from a fresh instance. Each writes `site/public/media/<name>-{light,dark}.{mp4,webm}`, and `Loop.astro` plays the pair on scroll with a still as the poster (`<Loop name="chat" poster="chat" />`).

| Scene | Media | The take |
|---|---|---|
| `loop-any-file` | `any-file` | A line typed at the end of the list in `To do.txt`, autosave, then `Server log.txt` opened |
| `loop-markdown` | `markdown` | A heading, two list items and a checkbox typed at the end of `Sourdough notes.md`, each rendering in place, then Source and back to Inline |
| `loop-search` | `search` | Search everywhere, `compost` typed, the Garden committee hit opened on its line |
| `loop-apps` | `apps` | Settings on Apps, Graph switched on, Settings closed: Nearby files in the Connections panel |
| `loop-graph` | `graph` | The Graph view of the folder, `garden` typed into its search so Garden plan and Garden committee 10 Sep stay lit, then cleared |
| `loop-versions` | `versions` | `Newsletter draft.md`, Revert To…, the second version selected and restored |

A take is cut from just before its first action to its last frame, kept 1320 px wide (twice the width the site draws a loop at), and encoded to mp4 under 1.2 MB and webm under 0.8 MB. Over a limit, the run steps the crf up by 4 twice, then drops to 24 fps and steps again, then stops. A loop take over 12 s also stops the run: a key waited for idle time, and the scene needs another run. Waiting for a panel or a button goes through the driver's accessibility lookup, `drive find <pid> <role> <name>`, rather than a fixed sleep.

A run of `loop-markdown` also encodes its takes, from the raw recording, as `docs/media/hero-{light,dark}.gif` for the README: palettegen then paletteuse without dithering, under 1.2 MB, at a lower frame rate and then a narrower width when over. The hero PNGs go once both GIFs exist. Every run ends by listing each loop and README GIF against its limit, as `--report` does, and exits 1 when one is over.

Still deferred to the announcement week: the hero loop (`summon`), the find band, the inbox and themes loops, and the Obsidian side-by-side. They come from the same harness, not from a hand-held take, and land by dropping the files in.
