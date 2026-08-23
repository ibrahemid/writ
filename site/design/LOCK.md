# Writ site: reference lock

Locked 2026-08-24 on branch site-surface-2026-08-22.

## Research

Live Refero MCP was unauthenticated in this session (OAuth URL issued, not completed). Research ran on the first-hand study already on disk: sixteen sites pulled as live HTML/CSS on 2026-08-22 (`marketer/reviews/2026-08-22/design-standard.md`, raw extractions under `raw/`) plus the fold/full captures in `marketer/reviews/2026-08-22/refs-tessera/` (raycast, cleanshot, things, zed, ghostty, ente), read as images. The audit that names every defect on the current page: `marketer/reviews/2026-08-22/audit-writ-site.md`.

Styles compared: raycast (dark stage, real inventory in the hero, CTA with platform detail), cleanshot (light desktop app, one loop per feature, prose narrow and media wide), things (narrowest container in the study, panorama breakouts, system type), shottr (hero overlap, CTA carries version and size), zed (hairline columns, moderate-weight display with negative tracking), tot (per-panel accent and full-bleed background).

## Primary reference: cleanshot.com, with the things.com panorama

Writ is a light desktop app on the macOS desktop. The light-page grammar with media that breaks the column is the one that matches the product (design-standard §1.6: match the product's own theme).

Preserve:
1. Text gets a measure (60ch), media never does. Screenshot strips and the live window run `wide` or `full`; text stays in `content`.
2. One feature, one capture. No icon grid, no three-card feature row.
3. Media cropped by the viewport edge (things `panorama-extension`, shottr overlap), never a centered card with a drop shadow.
4. CTA carries the operational detail: platform-detected label, version, file size, OS floor, architecture.
5. System-adjacent type: moderate display weight, negative tracking, two weights on the page plus a mono for commands and keys.

Borrow only:
- raycast: a dark full-bleed stage for one section where a screen recording needs it (the summon loop), and for the install band as the bookend. Nowhere else.
- zed: hairline rules and a 340-600 display weight range instead of 800.

## Token commitments (from the app's own design system, `site/design-system/colors_and_type.css`)

- Canvas: paper `#f6f4ef` light / `#0b0b11` dark. Dark band: `#0b0b11` in both polarities (the app's warp-dark surface), with a hairline, not a gradient.
- Accent: indigo `#3b5bdb` / `#8aa6ff`. This is the app's accent (ADR-008), not a site default; the gate will REVIEW it and the call is: keep, it is the brand. Role: links, the primary CTA, the caret, the active token. Never a background fill, never a decorative wash.
- Display: Bricolage Grotesque, `opsz` axis on, weight 600 (H1 640 max). Body: Inter 400, emphasis 600. Mono: JetBrains Mono 400/500 for commands, keys, file names. Three weights on the page (400/600 plus the display 600); the current 500/650/700/800 spread goes.
- Radius: 9px controls, 13px window shell. Hairline `var(--line)`. Shadow only on the window shell (`--win-shadow`), nothing else floats.
- Grid: the §6.1 named-line grid. `--content: min(1100px, 100% - gap*2)`, `--wide: min(var(--max-w-content), ...)` using the design system's `--max-w-content: 1340px` for wide, `--measure: 60ch`. Breakpoints upward at 1440 and 1920: gutters grow, wide grows to 1600, media keeps bleeding.

## Media strategy

Real captures only. No CSS-drawn windows, popovers, dialogs or terminals.
- Exists (docs/media): hero-light.gif, hero-dark.gif, command-palette.png, html-split.png, search-all-buffers.png. All v0.1-era and developer-seeded; used only where the content is not the point.
- Produced this pass from the real app (docs/design/2026-08-24/app/): stills of the non-developer buffers in split view, search everywhere, settings, the empty sidebar. These replace the v0.1 stills where the content is the point.
- Owed (site/docs/captures.md, exact specs): summon loop (hero), search loop, inbox loop, themes loop, Obsidian open comparison. Every owed loop has a slot with fixed aspect ratio, a real still as poster, and the play gated on intersection and reduced-motion.

## Copy rules

H1 names the job for a person. Every h2 states an outcome. "Markdown" is not a headline noun. Adjacency frame for Obsidian ("next to your vault"), never "alternative". No numbers that were not measured. Gate: `node ~/.claude/copy-gate/scan.mjs` plus the content-audit pass.

## Reject

- A single max-width wrapper for everything (the 1100px chain).
- `opacity: 0` until observed. Content renders without JS; motion is additive.
- Centered hero text over a gradient blob. Any radial glow.
- Cards as grouping. Cards only where the whole card is a link.
- Developer jargon in any seed, caption, or status bar shown to a visitor: no token counter, no Ln/Col, no `pytest`, no `.sql`.
- Serif-italic display, "//" eyebrows, generic taglines (design-taste.md).
- Averaging: the dark stage stays dark and bounded; the light page stays paper. No cream-and-clay drift.

## Directions explored

Three wireframes in `site/design/directions/`, rendered to `site/design/renders/directions/`. See the render critique in the session report. The chosen direction is recorded below once Ibra picks.

## Chosen direction

B, Stage (picked by Ibra 2026-08-24). Dark full-bleed hero band holding the split-pane conceit (source pane typing the page's own H1, rendered pane beside it) and the summon loop running off the band edge into the paper; paper body with edge-to-edge strips and left/right bleeds; tinted full band for search; dark install band as the bookend. Rejected: A (Desk, light throughout: reads closest to cleanshot, least identity), C (Sheet, page-as-window with a tab strip and a line-number gutter: the most distinctive and the one most likely to read as a developer tool to the Obsidian and non-developer audience).


## Build renders and critique (2026-08-24, final round)

Renders: `site/design/renders/final/` (index at 1440 fold/full, 1024, 390, plus 1920 fold and a dark 1440 fold; download, docs, vs/obsidian, guides/agent-output, changelog, press at the same set). Gate: design-gate scan 0 KILL; 9 REVIEW, all the same finding (uniform h2 rhythm inside doc prose and inside the island's demo document), judged fine: documentation headings are siblings and the island mirrors the app's own type. Copy gate clean. Checks: no element wider than the viewport at any breakpoint; nothing left at opacity 0 except the caret theatre and unplayed videos.

Critique against the lock, ten lines:
1. Text keeps its measure everywhere; the strips, window, search band and install band all break the column. Matches §1.1.
2. The hero is the product running (real capture in the slot, live island one scroll down), not an abstract visual. Matches §1.2-1.3.
3. H1 names the job in six words; every h2 states an outcome. Matches §1.4.
4. CTA carries version, OS floor, sizes; install band adds checksums, first-open, uninstall. Matches §1.7.
5. Rhythm varies: dark stage → paper → edge strip → wide window → left bleed → tinted band → right bleed → columns → dark bookend. No two adjacent sections share width and background.
6. No proof section, per the folivora call: nothing invented, silence instead.
7. Footer went from 5 links to 4 columns and 14 destinations; /vs, /docs, /guides, /press, /download all exist. Matches §1.9.
8. Weights on the page are 400/600 only; display carries hierarchy by size and opsz.
9. Open item: the summon loop is a still until the recording lands; the hero is honest but static. Highest-impact owed capture.
10. Open item: the "keeps" section's Finder still is an owed slot; the mechanism table carries the claim alone until then.
