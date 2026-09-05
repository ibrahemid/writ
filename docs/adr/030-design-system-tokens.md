# ADR-030: One token source for the app, the preview and the site

## Status

Accepted 2026-08-28. Supersedes ADR-008 in part (see Decision 8). Companion to
ADR-028 and ADR-029. Ships in 1.0, not in 0.4: release 0.4 is the storage
foundation and carries no design change.

## Context

Writ has three palettes and no single source for any of them.

The first is `src/styles/theme.css`, a 100-line `:root` block holding the
warp-dark values as literals, plus the font, spacing, radius and elevation
scales. The second is `src/styles/themes/*.json`, six presets whose colour
groups are flattened onto the same custom properties at runtime by
`src/stores/global/theme.ts`. The third is
`src-tauri/assets/preview-base.css`, 361 lines of hand-copied `--writ-preview-*`
values that mirror warp-dark by eye (`#0e0e14`, `#e0e0e0`, `#7aa2f7` at lines
20 to 26) with no mechanism tying them to the app tokens they mirror. The site
has a fourth, `site/design-system/colors_and_type.css`, dated 2026-06-24, on a
different brand (warm paper, indigo `#3b5bdb`, Bricolage Grotesque).

`flattenTheme` (`src/stores/global/theme.ts:20-30`) walks one level of the
preset JSON and writes every leaf to a `--writ-<group>-<key>` property. Both
`warp-light.json` and `warp-dark.json` carry a `site` block that nothing in
`src/` reads: `grep -rn "writ-site-" src/` returns zero hits. Under warp-dark
that block has 11 keys, so every boot writes 11 `--writ-site-*` properties to
`:root`, ten of them dead. The eleventh, `site.traffic`, is a nested object, and
`root.style.setProperty` (line 56) stringifies it, so the document root carries
`--writ-site-traffic: [object Object]`. `applyToRoot` mirrors the same snapshot
into `localStorage` under `writ-theme-vars` (line 60), and the pre-paint script
in `index.html` replays it, so the defect survives a restart. The preset
contract test already carves out the block by name
(`src/__tests__/stores/theme.test.ts:143-148`).

Below the colour layer there are no tokens at all for the things that carry the
most surface area. Radii: 38 raw px literals across `src/`, spread over twelve
distinct values (1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12 and 999), against 43
declarations that do use `var(--writ-radius-*)`, with seven more literals in
`preview-base.css`. Motion: 68 bare `ease` keywords across `src/` (32 followed
by a comma, 34 by a semicolon, two by `forwards`) against three `ease-out` or
`ease-in-out`, with no duration or easing token to name. Depth: fourteen
`z-index` declarations at ten unrelated values (3, 39, 40, 45, 100, 110, 150,
200, 210, 300).

The accepted baseline (`.status/design-baseline-2026-08-25.html`, all three OS
shells) moves the product off the terminal look and onto a warm, light-first
system with a per-platform chrome layer. Its "Changes" table names the current
code the baseline contradicts: the default preset is `"warp-dark"`
(`crates/writ-core/src/config/mod.rs:89`, `src/styles/themes/index.ts:20`);
prose is mono at 14px (`src/components/Editor/cm-theme.ts:16-17`); headings are
painted with the accent (`cm-theme.ts:123`); sidebar sections are 11px uppercase
with 1px tracking (`src/components/Sidebar/Sidebar.css:34-37`); the selected row
draws a 2px accent rail (`src/components/Sidebar/TabItem.css:24-32`); tabs are
bordered accent-tinted pills (`src/components/Editor/TabBar.css:34-35,51,54`); the
status bar always shows Ln/Col, language and UTF-8
(`src/components/Editor/StatusBar.tsx:69-71`); the palette sits behind a 0.5
scrim (`src/styles/theme.css:45`); the UI face is Inter at 14px
(`theme.css:75,77`); and the search glyph is a hand-drawn inline SVG
(`src/components/Sidebar/SearchBar.tsx:33`).

None of that is a per-file edit. Three palettes with no contract between them is
the reason a design pass never lands once, and the platform layer the baseline
asks for (three fonts, three window radii, three focus rings, three scrollbars)
has nowhere to live in the current shape.

## Decision

### 1. The tokens are DTCG JSON, and nothing else is a source

The source of truth is DTCG 2025.10 JSON under `design/tokens/`, a new
directory. No `.css`, `.ts` or `.rs` file is authored against a colour again.

- `base.json`: the spacing ramp (2 / 4 / 8 / 12 / 16 / 24 / 32), the radius
  scale, motion, z-index, and the UI and prose type scales.
- `warp-light.json`: the default scheme, carrying the light neutrals.
- `warp-dark.json`: the dark neutrals.
- `platform/mac.json`, `platform/win.json`, `platform/linux.json`: the per-OS
  overlay, which declares only the tokens that differ.
- `accent/*.json`: six accents (pine, writ-blue, terracotta, slate, plum, gold),
  each with a light and dark value, a hover, and a foreground per scheme.
- `preset/*.json`: the five existing presets (tokyo-night, catppuccin-mocha,
  dracula, solarized-dark, warp-dark) converted to the new schema and shipped as
  opt-in choices, so the terminal look stays reachable.

The build is Style Dictionary v5, run by a script in the root `package.json` and
by CI. Generated files are committed, and a CI check regenerates them and fails
when the working copy differs from the build output; a stale generated file is a
red build, not a silent drift.

Runtime keeps the `var(--writ-*)` indirection it has today. The theme store
applies a preset by setting custom properties, exactly as `applyToRoot` does
now, so live theme switching and the pre-paint restore in `index.html` keep
working. `flattenTheme` is fixed as part of this work: the `site` block leaves
both preset files, the nested-object path that produced
`--writ-site-traffic: [object Object]` is removed, and the flattener rejects a
non-string leaf instead of stringifying it.

The custom-property namespace changes with the schema. `--writ-surface-*`,
`--writ-foreground-*` and `--writ-border-*` are replaced by the baseline's
names: `--writ-bg-*`, `--writ-fg-*`, `--writ-border`, `--writ-border-soft`.

### 2. Four generated outputs, one of them for a consumer that does not exist yet

| Output | Consumer | Contents |
|---|---|---|
| `src/styles/generated/theme.css` | the app shell | `:root` for light, `[data-theme="dark"]` for dark, `[data-platform="mac"\|"win"\|"linux"]` for the platform overlay |
| `src/styles/generated/tokens.ts` | TypeScript | typed token constants, no CSS strings |
| `src-tauri/assets/generated/preview-tokens.css` | the preview iframe | the `--writ-preview-*` layer, replacing the palette block of `src-tauri/assets/preview-base.css` |
| the site target | `site/` | the same tokens in the form the Astro build consumes |

`src/components/Editor/cm-theme.ts` consumes `tokens.ts` rather than CSS
variables for the values CodeMirror needs as real values: `EditorView.theme` and
the `HighlightStyle` tag list. The syntax set is regenerated from the neutrals
at low saturation; Tokyo Night's values stay as the tokyo-night preset.

`preview-base.css` loses its palette block and keeps only rules, which reference
`var(--writ-preview-*)`. The palette existed because the preview document is a
separate origin that cannot read the app's `:root`; the generated file, inlined
before the rules in every served document, solves the same problem without a
second hand-maintained palette. The
contract test `src/__tests__/styles/preview-base-contrast.test.ts` is re-pointed
at the generated file and keeps asserting AA in both schemes.

The site target exists from day one and is built by the same script, even though
the site rebuild lands later. The point of a token contract is that the second
consumer cannot fork it; producing the target now is what prevents
`site/design-system/colors_and_type.css` from being reinvented.

### 3. Type, colour, radius, shadow, motion

Values are the baseline's, copied as they stand, with one correction. The baseline's faint
tier (`#9A948B` light, `#6F6A60` dark) clears 3:1 on the canvas only; on the sidebar, sunken and
selected surfaces it measures 2.54 to 2.78 and 2.42 to 3.36, below the 3:1 floor the preset
contrast test has enforced on every surface since 2026-06-11. The faint tier is darkened to
`#8A847A` and `#827C72`, which measure 3.14 to 3.71 and 3.14 to 4.37 across the same surfaces.

**Fonts**

| Token | Value |
|---|---|
| `--writ-font-ui` | `-apple-system, BlinkMacSystemFont, "Segoe UI Variable Text", "Segoe UI", "SF Arabic", "Geeza Pro", "Noto Naskh Arabic", system-ui, sans-serif` |
| `--writ-font-prose` | `var(--writ-font-ui)` |
| `--writ-font-prose-alt` | `"iA Writer Quattro S", var(--writ-font-ui)` |
| `--writ-font-mono` | `ui-monospace, SFMono-Regular, "JetBrains Mono", "SF Mono", Menlo, Consolas, monospace` |

The Arabic families keep the position the shipped stack gives them (`src/styles/theme.css:75`), after the Latin
faces and before the generic keyword, so per-glyph fallback still reaches SF
Arabic, Geeza Pro or Noto Naskh Arabic before the platform default.

**UI scale**

| Token | Size | Line height | Used for |
|---|---|---|---|
| `--writ-ui-xs` | 11px | 14px | Captions, keycaps |
| `--writ-ui-sm` | 12px | 15px | Meta, word count, section headers |
| `--writ-ui-md` | 13px | 16px | Sidebar rows, tabs, menus |
| `--writ-ui-lg` | 15px | 20px | Pane headers |
| `--writ-ui-tracking` | 0 | | Never letter-space UI labels |

**Prose scale**

| Token | Value | Weight | Line height | Notes |
|---|---|---|---|---|
| `--writ-prose-size` | 16px | 400 | 1.6 | System sans, not mono |
| `--writ-prose-measure` | 704px | | | 44rem at a 16px root; the app root is 13px, so the token is absolute |
| `--writ-prose-pad-x` / `-y` | 32px | | | |
| `--writ-prose-p-spacing` | 1.25rem | | | |
| `--writ-heading-spacing` | 2em | | | Space above a heading |
| `--writ-h1-size` | 1.75em | 700 | 1.2 | Also the inline title |
| `--writ-h2-size` | 1.40em | 650 | 1.25 | |
| `--writ-h3-size` | 1.20em | 600 | 1.3 | |
| `--writ-h4-size` | 1.05em | 600 | 1.4 | |
| `--writ-h5-size` | 1.00em | 600 | 1.5 | |
| `--writ-h6-size` | 1.00em | 600 | 1.5 | |
| `--writ-heading-color` | inherit | | | Never the accent |
| `--writ-heading-formatting` | `var(--writ-fg-faint)` | | | The hung `#` markers |

Markdown markers stay visible and hang in the left margin at
`--writ-fg-faint`, so the sentence itself never carries raw punctuation.

**Light neutrals**

| Token | Value | Contrast on canvas | Used for |
|---|---|---|---|
| `--writ-bg-canvas` | `#FFFFFF` | 1.00 | Editor surface |
| `--writ-bg-sidebar` | `#F7F6F3` | 1.08 | Sidebar, lighter than the canvas |
| `--writ-bg-raised` | `#FFFFFF` | 1.00 | Popovers, menus, sheets |
| `--writ-bg-sunken` | `#F1EFEB` | 1.15 | Inputs, code blocks, chips |
| `--writ-bg-hover` | `rgba(28, 26, 23, .055)` | 1.12 | Row and tab hover |
| `--writ-bg-selected` | `rgba(28, 26, 23, .085)` | 1.18 | Selected row, neutral |
| `--writ-border` | `#E4E1DB` | 1.31 | Table rules, blockquote |
| `--writ-border-soft` | `#EFEDE8` | 1.17 | Sidebar and toolbar hairlines |
| `--writ-fg` | `#1C1A17` | 17.36 | Body and heading ink |
| `--writ-fg-muted` | `#5D5850` | 7.05 | Section headers, icons, meta |
| `--writ-fg-faint` | `#8A847A` | 3.71 | Hung markers, placeholders |

**Dark neutrals**

| Token | Value | Contrast on canvas | Used for |
|---|---|---|---|
| `--writ-bg-canvas` | `#1B1A18` | 1.00 | Editor surface |
| `--writ-bg-sidebar` | `#201F1C` | 1.06 | Sidebar |
| `--writ-bg-raised` | `#262521` | 1.13 | Popovers, menus, sheets |
| `--writ-bg-sunken` | `#171614` | 1.04 | Inputs, code blocks, chips |
| `--writ-bg-hover` | `rgba(255, 252, 246, .06)` | 1.18 | Row and tab hover |
| `--writ-bg-selected` | `rgba(255, 252, 246, .10)` | 1.34 | Selected row, neutral |
| `--writ-border` | `#333029` | 1.32 | Table rules, blockquote |
| `--writ-border-soft` | `#272521` | 1.14 | Sidebar and toolbar hairlines |
| `--writ-fg` | `#EDEAE3` | 14.47 | Body and heading ink |
| `--writ-fg-muted` | `#A8A296` | 6.85 | Section headers, icons, meta |
| `--writ-fg-faint` | `#827C72` | 4.20 | Hung markers, placeholders |

**Accents**

| Accent | Light | On `#FFF` | Hover | Dark | On `#1B1A18` | Hover | Fg light | Fg dark |
|---|---|---|---|---|---|---|---|---|
| Pine, default | `#1F6F5C` | 6.02 | `#1C6453` | `#5BC5A7` | 8.27 | `#6BCBB0` | `#FFFFFF` | `#10221D` |
| Writ blue, the app today | `#3B5BDB` | 5.67 | `#3552C5` | `#7AA2F7` | 6.91 | `#87ABF8` | `#FFFFFF` | `#151C2A` |
| Terracotta | `#A8492B` | 5.75 | `#974227` | `#E08A5F` | 6.60 | `#E3966F` | `#FFFFFF` | `#261710` |
| Slate | `#2F5F9E` | 6.46 | `#2A568E` | `#8FB6EA` | 8.32 | `#9ABDEC` | `#FFFFFF` | `#181F28` |
| Plum | `#7B3F86` | 7.25 | `#6F3979` | `#C79BD1` | 7.48 | `#CDA5D6` | `#FFFFFF` | `#221A24` |
| Gold | `#8A6200` | 5.49 | `#7C5800` | `#D9B24C` | 8.63 | `#DDBA5E` | `#FFFFFF` | `#251E0D` |
| Destructive, derived | `#A6392E` | 6.47 | | `#E08D82` | 6.87 | | | |

Hover is 10% darker in light and 10% lighter in dark. Contrast on the canvas
runs 5.49 to 7.25 in light and 6.60 to 8.63 in dark. The destructive red and the
toggle off-track are derived from the neutrals, not authored.

**Radii**

| Token | Value | Applies to |
|---|---|---|
| `--writ-r-row` | 6px | Sidebar rows, list rows, menu items |
| `--writ-r-control` | 6px | Buttons, inputs, selects, keycaps, checkboxes |
| `--writ-r-tab` | 6px | Top corners only when docked |
| `--writ-r-popover` | 10px | Menus, tooltips |
| `--writ-r-modal` | 12px | Dialogs, settings, command palette |
| `--writ-r-card` | 12px | Note cards, callouts |
| `--writ-r-media` | 6px | Images, code blocks, thumbnails |
| `--writ-r-pill` | 999px | Tags, status chips, the search field |
| `--writ-r-window` | 10px | The window itself |

**Shadows**

| Token | Scheme | Value |
|---|---|---|
| `--writ-shadow-popover` | Light | `0 1px 6px rgba(0,0,0,.05), 0 6px 24px rgba(0,0,0,.09)` |
| `--writ-shadow-modal` | Light | `0 2px 10px rgba(0,0,0,.07), 0 16px 48px rgba(0,0,0,.13)` |
| `--writ-shadow-chip` | Light | `0 1px 3px rgba(0,0,0,.06)` |
| `--writ-shadow-popover` | Dark | `0 1px 6px rgba(0,0,0,.16), 0 6px 24px rgba(0,0,0,.28)` |
| `--writ-shadow-modal` | Dark | `0 2px 10px rgba(0,0,0,.22), 0 16px 48px rgba(0,0,0,.40)` |

**Icons, motion, chrome metrics**

| Token | Value | Notes |
|---|---|---|
| `--writ-icon-size` | 20px | Phosphor Regular, one weight everywhere |
| `--writ-icon-color` | `var(--writ-fg-muted)` | Accent only on the selected row |
| `--writ-icon-opacity` | 0.85 | Rises to 1 on hover |
| `--writ-motion` | 120 to 180ms `cubic-bezier(.33, 1, .68, 1)` | Fills and opacity only |
| `--writ-sidebar-width` | 240px | Resizable 200 to 320 |
| `--writ-sidebar-row-pitch` | 28px | Fill 26px, 1px margin |
| `--writ-space-7` | 32px | Added to the 2 / 4 / 8 / 12 / 16 / 24 ramp |

Motion animates fills and opacity. Transform and layout animation are not in the
system, and the 68 bare `ease` keywords are replaced by `var(--writ-motion)`.

**Z-index**

The baseline has no depth scale, and the app has ten unrelated literals. The
scale is seven named layers, and a component picks a layer rather than a number:

| Token | Value | Layer |
|---|---|---|
| `--writ-z-base` | 0 | Editor, sidebar, in-flow chrome |
| `--writ-z-chrome` | 100 | Sticky headers, the toolbar, the tab strip |
| `--writ-z-popover` | 200 | Menus, tooltips, the spelling popover |
| `--writ-z-palette` | 300 | The command palette sheet |
| `--writ-z-modal` | 400 | Settings, dialogs, the theme editor |
| `--writ-z-toast` | 500 | Toasts and the update banner |
| `--writ-z-drag` | 600 | The drag image and the resize handle |

### 4. The platform layer is a token overlay, not a fork

The root element carries `data-platform="mac"`, `"win"` or `"linux"`, set once
at boot in `App.tsx` from the platform the webview reports (`src/lib/platform.ts`,
which already reads `navigator.platform` for the title bar), with a development
override so all three shells can be rendered on one machine. Writ is
single-window, so the attribute is written once and never recomputed. The platform token files
declare only what differs; everything absent from an overlay falls through to
the base.

| Token | macOS | Windows | Linux |
|---|---|---|---|
| UI font | `-apple-system`, `BlinkMacSystemFont` | Segoe UI Variable Text | Adwaita Sans |
| Mono font | SF Mono | Cascadia Code | Adwaita Mono |
| UI base size | 13 / 16 | 14 / 20, emphasis is Semibold at the same size | 14.67 / 20.5 |
| Small label | 11 and 12 | 12 / 16, the floor | 12.03 / 16.8 |
| Prose size | 16 / 1.6 | 16 / 24 | 16 / 140% |
| Title bar height | None; 44px sidebar head | 32, or 48 when it holds a search box | 47, or 37 with `.default-decoration` |
| Controls side | Left | Right | Right |
| Controls geometry | 3 x 12px circles, 8px gap | 3 x 46 x 32px squares, radius 0, glyph box 10 | Close only, 24px circle, 16px icon, 34px hit box |
| Window radius | 10 | 8, and 0 when maximized or snapped | 15, and 0 when maximized, tiled or fullscreen |
| Control radius | 6 | 4 | 9 |
| Popover radius | 10, modal 12 | 8, tooltip 4 | 15, card 12 |
| Focus ring | 1px accent border plus a 3px accent halo | 2px outer plus 1px inner, inverted, 3px outside the control | 2px accent at 50%, offset -2px, inside the control |
| Scrollbar | Overlay, native | 12px rail, thumb 2 to 6px, radius 3, min 30, 83ms | Overlay, 3px idle to 8px hovered, radius 99, 200ms linear |
| Selection indicator | Neutral fill, weight 500, accent icon tint | Neutral SubtleFill plus a 3 x 16px accent bar on the left edge | Neutral currentColor 10% fill, accent icon tint |
| Shadow alphas | Modal 0.07 and 0.13 light, 0.22 and 0.40 dark | Ambient 0.12 plus key 0.14 light, 0.24 plus 0.28 dark, always with a 1px stroke | 0.15, 0.10 and a `0 0 0 1px 5%` hairline, painted by the app |
| Accent source | `--writ-accent`, app setting | Windows Settings; Dark1 in light, Light2 in dark | gsettings or portal, nine values, blue default |
| Font smoothing | antialiased, grayscale | None; DirectWrite | None; slight hinting, grayscale |

These rows are identical on all three platforms and belong to `base.json`:

| Token | Value |
|---|---|
| Spacing ramp | 2 / 4 / 8 / 12 / 16 / 24 / 32 |
| Prose measure | 44rem, centred |
| Prose padding | 32px on both axes |
| Paragraph spacing | 1.25rem |
| Heading spacing | 2em above |
| Heading colour | inherit |
| Markdown markers | Hung left at `--writ-fg-faint` |
| Neutral hue | Warm, ink `#1C1A17`; Windows keeps the neutral text and stroke alphas, Linux keeps the libadwaita alphas |
| Sidebar width | 240px, resizable 200 to 320 |
| Icon set | Phosphor Regular, one weight |
| Sections | Open, Recent, folder name |
| Word count | Top right of the editor |
| Accent spend | Links, caret, checked box, focus, one primary button |
| Child row indent | 16px past the parent label |

Icons are Phosphor Regular at `--writ-icon-size`, 20px in chrome, with the
Windows overlay dropping sidebar rows and the toolbar to 16px to sit on the
Fluent grid. One weight everywhere.

Two pieces of chrome are components built on top of these tokens rather than
tokens themselves. The Windows caption buttons: three 46 x 32px squares at the
right, radius 0, a 10 x 10px glyph box, `ChromeMinimize E921`,
`ChromeMaximize E922`, `ChromeRestore E923`, `ChromeClose E8BB`, minimize and
maximize hover on `SubtleFillColorSecondary`, close hover on a `#C42B1C`
backplate with a `#FFFFFF` glyph, and the 3 x 16px accent selection pill on the
sidebar row's left edge. The GNOME header bar: one 47px bar with the toolbar
merged into it, New note and the title in the bar, search in the sidebar's own
segment, and a single circular close button at the right from
`button-layout 'appmenu:close'`.

### 5. Defaults, polarity and the surfaces that change with them

- **Light is the default.** `theme.preset` defaults to the light scheme instead
  of `"warp-dark"`, and follow-system is on: the app takes the OS light or dark
  setting unless the user pins a preset. Warp Dark and the four terminal presets
  stay as opt-in choices, so the current look is one setting away.
- **Dark is the baseline dark neutrals**, not the old `#0e0e14` palette.
- **Accent is pine**, with writ-blue, terracotta, slate, plum and gold offered
  in Settings.
- **The status bar is off by default**, behind `editor.status_bar`. When on it
  carries Ln/Col, language, encoding and word count. When off, the word count
  shows at the top right of the editor.
- **The command palette** has no scrim: a shadowed sheet over the note, 640px
  wide, a 40px input, 32px rows.
- **The tab strip is hidden at one note.** Tabs are borderless, 28px on a 36px
  bar, and the active tab carries the canvas colour instead of an accent tint.
- **The sidebar** is 240px, rows on a 28px pitch with a 26px fill, sections in
  sentence case at `--writ-ui-sm` with tracking 0, named as plain nouns (Open,
  Recent, Search results, and the folder's own name), selection as a neutral
  `--writ-bg-selected` fill with weight 500 and the accent on the row icon only,
  child rows indented 16px past the parent label.
- **The toolbar** is 44px: sidebar toggle, New note, the formatting cluster, and
  the search field.
- **The empty sidebar** offers New note and Open a folder as buttons with their
  keycaps beside them, under "No notes yet."

### 6. Where the accent is allowed to appear

The accent is spent on links, the caret, a checked box, the focus ring, one
primary button, and the selected row's icon. On Windows the 3 x 16px selection
indicator carries it instead of the icon tint.

It is never spent on headings, tab fills, selection fills, or a sidebar rail.
Weight and size carry hierarchy; colour carries interaction. The destructive red
is derived from the neutrals rather than taken from the accent set, so a
destructive action reads the same whichever accent is chosen.

### 7. Fonts: the system face, one alternate, mono for code

UI and prose take the system stack for the platform. The one alternate prose
face is iA Writer Quattro S, offered in Settings and bundled with the app under
the SIL Open Font License: it ships unmodified and keeps its reserved font name,
so it is referenced as `iA Writer Quattro S` and never renamed or subset in a
way that would require a new name.

Mono is for code: fenced blocks, inline code, and the terminal-shaped surfaces
that quote a command. It is not the editor's body face and not a decorative
signal anywhere in chrome.

Three webfonts leave the app: Inter, JetBrains Mono and Bricolage Grotesque.
`src/styles/fonts.css`, whose three lines are the `@fontsource/inter` latin
imports, is deleted along with the `@fontsource/inter` dependency; nothing
replaces it, because the system stack needs no `@font-face`. The one
`@font-face` the app keeps is the bundled Quattro S. The site's copies under
`site/design-system/fonts/` go with the site rebuild, not before it, since the
current site still renders from them.

### 8. What this supersedes in ADR-008, and what survives it

Superseded:

- **Inter as the UI face.** The face is the platform's own, so chrome now looks
  like macOS on macOS, Windows on Windows, and GNOME on GNOME. ADR-008 took the
  opposite trade deliberately, buying identical chrome across the three
  platforms for ~75 KB; the baseline reverses it, because a notes app that
  ignores its host reads as a web page.
- **The self-hosted webfont decision** and the `@fontsource/inter` dependency.
  The privacy property ADR-008 protected (no CDN call, no font-host telemetry)
  is preserved by construction: a system stack makes no network request either.
- **Two font tokens.** There are four: `--writ-font-ui`, `--writ-font-prose`,
  `--writ-font-prose-alt`, `--writ-font-mono`. `--writ-font-sans` is gone; UI
  surfaces take `--writ-font-ui`.
- **Mono at the CodeMirror editor.** ADR-008 named the editor as one of two
  permitted mono sites; the editor is now prose sans at 16px on a 1.6 line. The
  second permitted site, sidebar history timestamps, goes with it.

Kept:

- **Mono is opt-in and scoped**, narrowed from ADR-008's two sites to one rule:
  code. The reasoning is unchanged, that the visual signal of mono is diluted by
  every non-code use.
- **No literal font stack in a stylesheet.** `font-family` is a token or
  `inherit`, and `src/__tests__/styles/typography-tokens.test.ts` keeps enforcing
  it against the new token names.

### 9. The gate

- Every design-system pull request attaches screenshots of the macOS, Windows
  and Linux shells. A change that cannot be shown on all three is not finished.
- New or materially changed UI gets a design review at real breakpoints; new or
  changed user-visible strings get a copy review against the plain-word rules of
  ADR-028 section 10.
- A grep-based test fails on a literal colour (hex, `rgb(`, `rgba(`, `hsl(`) in
  `src/` or `site/` outside `src/styles/generated/` and the site's generated
  target. It lands with the last retokenisation pull request, not before, so it
  never ships as a permanently skipped test.
- The generated-file check described in Decision 1 runs in the same job.
- `src/components/ThemeEditor/` keeps working: it edits token overrides through
  `themeStore.setOverride` and is re-pointed at the new token groups, so a user
  can still recolour the app and still reset to the preset.

## Consequences

- Writ opens light, with sans prose at 16px, no status bar and no tab strip at
  one note. For anyone used to the current build this is a different product on
  first paint. The Warp Dark preset plus `editor.status_bar` restores the old
  reading closely enough that the change is reversible in Settings, and the four
  terminal presets are unchanged in spirit even though their JSON is rewritten
  to the new schema.
- Existing configs are not lost: a config naming `warp-dark` keeps getting
  warp-dark, because the preset survives the conversion under the same id. Only
  the default for a config that names nothing changes.
- Per-token overrides in `theme.overrides` are keyed by the old group names
  (`surface.background`, `accent.default`), and the schema change invalidates
  them. Overrides that no longer resolve are dropped on load rather than
  applied, and the theme editor shows the preset value.
- The frontend gains a build step. Editing a colour now means editing JSON and
  running the token build, and a pull request that edits a generated file by
  hand fails CI. That cost is the point: it is what makes a second consumer
  impossible to fork.
- `src-tauri/assets/preview-base.css` is deleted, which removes the only place
  where the preview palette could drift from the app. The preview iframe itself
  is untouched: it stays mounted for the life of the app, and swapping a
  stylesheet URL is not an unmount.
- The site inherits the tokens rather than a screenshot of them. The rebuild
  under `site/` replaces `site/design-system/colors_and_type.css` and its three
  bundled families with the generated target, and the site's brand becomes the
  app's, which it has never been.
- Reading the system accent is not shipped. Windows exposes it through
  `UISettings.GetColorValue` plus a `ColorValuesChanged` listener and GNOME
  through gsettings or the xdg portal, and Tauri v2 surfaces neither, so
  following the OS accent needs a plugin or a platform command that does not
  exist yet. The six accents are the shipped set; follow-system accent is a
  later addition and does not block 1.0.
- The dark `--writ-shadow-chip` is absent from the baseline table. It is derived
  from the light chip value against the dark modal alphas when the first chip
  component is built, and recorded in `design/tokens/warp-dark.json` at that
  point.
- `default_sidebar_open` returns `false`
  (`crates/writ-core/src/config/mod.rs:41`) while every baseline shell shows the
  sidebar open. The default flips with this work, so a first launch shows the
  notes list rather than an empty canvas.
- `src/__tests__/stores/theme.test.ts` loses its `COMPOSITE_TOKENS` carve-out
  (lines 143-148) when the `site` block goes, and gains the stricter rule that
  every token leaf is a string.
