# Capture list for the site revamp

Status 2026-08-24: the split-view, search, mermaid, KaTeX, HTML, sidebar-history and settings stills now come from the real app (docs/design/2026-08-24/app/, copied into site/src/assets/captures/). The five loops below and the Finder still are still owed. The hero (`summon`) and the find band (`search`) are live Loop slots: drop `site/public/media/<name>-{light,dark}.{webm,mp4}` in and they play on scroll with the current still as the poster (site/src/components/site/Loop.astro). Shots 3-5 currently render as stills; wire a Loop where the capture lands.

Owed, in order of impact:

1. Shot 1 (summon loop) — the hero slot. Record the window at 1100x720 (the hero box is locked to that ratio).
2. Shot 2 (search loop) — the find band; the still-search capture stands in.
3. Shot 3 (inbox loop) — the agent-output guide header; still stands in.
4. Shot 4 (themes loop) — the index section; theme swatches stand in.
5. Shot 5 (Obsidian side-by-side) — /vs/obsidian; the still-vault capture stands in.
6. still-finder-buffers — Finder open on ~/.writ/buffers, one plain file per note, no identifying paths. Used by the "It doesn't lose things" section (currently an owed slot).

Six recordings for `site/public/media/`. Five loops and one set of stills. Shot 1 is the hero.

Encode targets, budgets and the autoplay rules come from the design standard: hero mp4 under 1.2 MB, webm under 0.8 MB, poster under 60 KB, page under 1.8 MB on first view.

## Before recording

**The app.** Latest build, `~/.writ` reset to a clean profile so no real note appears. Seed the buffers to match `site/src/islands/writ/buffers.ts`: tabs `meeting-notes.md`, `trip-packing.md`, `recipe.md`, `draft-email.md`, with `notes.md`, `reading-list.md`, `newsletter.html`, `settle.ts` in history. The page and the video have to show the same window.

**Window.** 1280x800 on a Retina display, so the native capture is 2560x1600. Sidebar open. Editor font at the default zoom. Spelling off, Rewrite off, update banner dismissed.

**Themes.** Every shot runs twice, once on `warp-light` and once on `warp-dark` (Settings, `Cmd+,`, Appearance). Same take, same timing, both passes.

**Screen Studio.** Record at 2x the display slot. Cursor smoothing on. Click zoom **off** for shots 1 to 4; the zoom pan reads as a tutorial. Export tab: "Custom", not "Web", width set explicitly per shot. No background wallpaper effects, no rounded-corner shadow padding beyond the default.

**Desktop hygiene.** Shot 1 shows other apps. Use a browser on a public page and a Slack workspace with no real names, or blur the message list in the edit. Menu bar clock set, notifications off (`Do Not Disturb`), no dock badges.

## Encode

Run per file. `NAME` is the shot's file stem, `W` is its export width.

```sh
ffmpeg -i raw.mov -an -vf "scale=$W:-2" -c:v libx264 -profile:v high -pix_fmt yuv420p \
  -crf 24 -preset slow -movflags +faststart "$NAME.mp4"
ffmpeg -i raw.mov -an -vf "scale=$W:-2" -c:v libvpx-vp9 -crf 34 -b:v 0 -row-mt 1 "$NAME.webm"
ffmpeg -i "$NAME.mp4" -frames:v 1 -q:v 2 "$NAME-poster.png" && cwebp -q 82 "$NAME-poster.png" -o "$NAME-poster.webp"
```

`-movflags +faststart` and `-an` are not optional. The poster must come from frame 0 of the exact mp4 that ships, or the swap flashes. Stills go through `cwebp -q 90`; if the text blurs at 1x, ship PNG instead.

## Shot 1: hotkey summon

**Before.** Full desktop, no Writ window. A browser at the front with a real page open, Slack behind it, both large enough to read. Menu bar visible.

**Take.** Hold two seconds on the desktop. Press `Cmd+Shift+Space`. Writ appears over both windows with the four tabs already loaded, `meeting-notes.md` active in split view. Hold four seconds without moving the cursor. Press `Esc`. Hold one second on the bare desktop.

**Duration.** 6 s, loops. R2 sets 6 s. If this becomes the hero rather than a section loop, re-record at 8 s (the standard's floor) by holding two extra seconds on the open window.

**Variants.** `summon-light`, `summon-dark`. Same desktop arrangement in both, macOS appearance switched to match.

**Size.** Full-bleed. Export width 1600. Budget 1.2 MB mp4, 0.8 MB webm.

**Files.** `summon-light.mp4` `.webm` `summon-light-poster.webp`, same for `-dark`.

**The poster carries the message on its own.** A share of iPhone visitors never see the video, so frame 0 has to be the moment Writ is already over the other windows with tabs readable, not the empty desktop. Trim the two-second lead-in from the encode and put it back with a `loop` gap if the pause matters.

## Shot 2: search everywhere

**Before.** Writ open, `meeting-notes.md` active, a workspace folder open in the sidebar with ten or so files in the tree.

**Take.** Press `Cmd+Shift+F`. Type `radiator` one character at a time at a human speed. Results fill in as it is typed: matches across open buffers, buffers from history, and workspace file names and contents, sectioned. Pause two seconds on the full result list. Arrow down three rows, `Enter` on a workspace file, and land on the matched line.

**Duration.** 10 s.

**Variants.** `search-light`, `search-dark`.

**Size.** Section width. Export width 1200. Budget 800 KB per file.

**Files.** `search-light.mp4` `.webm` `search-light-poster.webp`, same for `-dark`.

**Note.** The result count and the millisecond timing in the corner are the point of the shot. Do not crop them out.

## Shot 3: watched inbox

**Before.** Settings, Files, "Watched inbox folder" pointed at a folder. Writ open on any buffer with the sidebar showing that folder, empty. A Finder window or a terminal visible in a strip at the side.

**Take.** Hold two seconds. Drop a file into the folder from Finder, or run a command that writes it. The file appears in the sidebar and opens by itself, rendered. Hold three seconds on the rendered file.

**Duration.** 8 s.

**Variants.** `inbox-light`, `inbox-dark`.

**Size.** Section width. Export width 1200. Budget 800 KB per file.

**Files.** `inbox-light.mp4` `.webm` `inbox-light-poster.webp`, same for `-dark`.

**Note.** The arriving file should be something a machine would write: a report with a heading and a table, not a one-line text file. It has to be worth rendering.

## Shot 4: themes

**Before.** Writ open on `recipe.md` in split view. Settings closed.

**Take.** `Cmd+,`, Appearance, click through the six presets in order: `warp-light`, `warp-dark`, `tokyo-night`, `catppuccin-mocha`, `dracula`, `solarized-dark`. Roughly one second on each. Close settings on the last one.

**Duration.** 8 s.

**Variants.** One file. The shot is the theme change, so it carries both polarities itself. Poster frame from `warp-light`.

**Size.** Section width. Export width 1200. Budget 800 KB.

**Files.** `themes.mp4` `.webm` `themes-poster.webp`.

**Note.** If the settings modal covers too much of the buffer, drive the theme change from the command palette instead and keep the buffer in full view.

## Shot 5: opening an Obsidian vault

**Before.** An Obsidian vault folder with real structure, at least 200 notes. Obsidian fully quit. Writ resident, window hidden.

**Take.** Split screen. Left: launch Obsidian from the Dock and open one note in the vault. Right: `Cmd+Shift+Space`, open the same file in Writ through the file open path. Run both from the same starting state in one take, with a visible timer.

**Duration.** 12 s.

**Variants.** `obsidian-open-light`, `obsidian-open-dark`.

**Size.** Section width. Export width 1200. Budget 800 KB per file.

**Files.** `obsidian-open-light.mp4` `.webm` `obsidian-open-light-poster.webp`, same for `-dark`.

**Honesty.** Writ is resident and Obsidian is being launched. That is the claim, so the caption on the page has to say it in those words. A cold Writ launch against a cold Obsidian launch is a different number and this shot is not it. If the timer is on screen, it has to be a real clock, not one added in the edit.

## Shot 6: stills

Four screenshots, light and dark, at 2x the slot they sit in. `cwebp -q 90`, PNG if the text softens. Budget 150 KB each, 400 KB ceiling.

| File | On screen |
|---|---|
| `still-split-note` | `meeting-notes.md` in split view, source left, rendered right, the action checkboxes visible and one of them ticked |
| `still-mermaid` | `writ-4471` rendered, the moving-day flowchart, source pane showing the fenced block |
| `still-katex` | `writ-3182` rendered, the loan formula set as display math |
| `still-html` | `newsletter.html` in split view, the rendered newsletter on the right |

Same window size and same sidebar state across all four, so they can sit in a row without jumping.

## Checklist per file

- [ ] Both polarities recorded from the same take direction
- [ ] Exported at the stated width from Screen Studio's Custom tab
- [ ] mp4 and webm both produced, audio stripped, `+faststart` set
- [ ] Poster pulled from frame 0 of the shipped mp4
- [ ] Under budget, checked with `ls -lh`
- [ ] Text still readable at 1x
- [ ] Nothing personal on screen: no real names, no real message content, no file paths with a home directory that identifies anyone
