# ADR-008: Typography Tokens

**Status:** Accepted
**Date:** 2026-05-21

## Context

Writ started with a single typography token, `--writ-font-mono`, and a body
default of mono. CodeMirror is mono by nature, so applying mono everywhere was
the path of least resistance. As chrome surfaces grew (sidebar, command palette,
tab strip, status bar, modals, settings, shortcut editor, update banner), the
all-mono UI began to read as a code dump rather than a polished editor shell.
Chrome and content were typeset identically, and there was no way to distinguish
them without changing every surface in lockstep.

A second issue: several components (`Kbd`, `ThemeEditor`, `TabBar` rename input)
declared `font-family: var(--writ-font-mono)` explicitly, while most surfaces
relied on inheritance. That asymmetry made any future typography change a
multi-file edit instead of a token flip.

## Decision

### Two tokens, sans default

Define exactly two typography tokens in `src/styles/theme.css`:

- `--writ-font-sans` — `"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI",
  system-ui, sans-serif`. Inter is the primary face; the system stack is the
  fallback while Inter is loading or unavailable.
- `--writ-font-mono` — existing JetBrains Mono stack.

Body uses sans. Every chrome surface inherits sans for free. Mono is opt-in via
explicit `font-family: var(--writ-font-mono)`.

### Mono is scoped to two places

Mono is permitted only at:

1. The CodeMirror editor (`src/components/Editor/cm-theme.ts` — `&` and
   `.cm-scroller`).
2. Sidebar history timestamps (`.tab-item-trailing` in
   `src/components/Sidebar/TabItem.css`, set only by `HistorySection`).

Every other component — including hex codes, keyboard chips, color values,
filenames, rename inputs — uses sans. The visual signal of mono is reserved for
actual code content and machine-formatted relative timestamps; using it anywhere
else dilutes that signal.

### No hardcoded font-family

CSS files must never declare a literal font stack. `font-family` must be one of:
`var(--writ-font-sans)`, `var(--writ-font-mono)`, or `inherit`. A regression
test (`src/__tests__/styles/typography-tokens.test.ts`) scans the repo and fails
on any other value.

### Self-hosted webfont, no network fetch

Inter ships bundled with the app via `@fontsource/inter` (latin subset, weights
400/500/600). The woff2 files are emitted into `dist/assets/` at build time and
served from the local origin by Tauri's webview. There is no CDN call on
launch, no external dependency at runtime, and no telemetry leak from font
hosting. The system stack remains as a fallback while the woff2 is loading
(`font-display: swap`) and on the very first paint before the woff2 has been
parsed. Total added weight: ~75 KB across three latin woff2 files.

This trades a small binary increase for editorial consistency across platforms:
chrome looks identical on macOS, Windows, and Linux instead of resolving to
three different system fonts.

## Consequences

**Positive:**
- One source of truth for chrome typography. Changing the sans stack is a
  one-line edit in `theme.css`.
- Chrome and editor content are visually distinct without any per-component
  work.
- The regression test makes drift loud: any new component that hardcodes a font
  stack breaks the test.

**Negative / risks:**
- Hex codes and keyboard chips lose mono's tabular alignment. Mitigated where
  needed via `font-variant-numeric: tabular-nums` (already used on
  `.active-group-count` and `.tab-item-trailing`).
- Bundle grows by ~75 KB (three latin woff2 files). Accepted in exchange for
  identical chrome across macOS, Windows, and Linux.
- A future non-latin subset (cyrillic, greek, vietnamese) will require swapping
  to the full `@fontsource/inter/{weight}.css` imports or adding the subset
  files explicitly. Until then non-latin glyphs fall back to the system stack.
