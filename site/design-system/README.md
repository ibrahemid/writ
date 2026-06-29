# Writ Design System

Brand: warm paper, single indigo accent, ink.

## Identity

- **Background:** warm paper `#f6f4ef` (light) / deep midnight `#0b0b11` (dark)
- **Foreground:** near-black ink `#1a1a22` (light) / `#e0e0e0` (dark)
- **Accent:** indigo `#3b5bdb` (light) / `#8aa6ff` (dark). One accent, everywhere. The accent dot in the wordmark "Writ." is the brand punctuation.
- **Semantic greens/oranges/reds:** status only (`--ok`, `--warn`, `--err`). Never as brand.

## Fonts (self-hosted, no CDN)

| Role | Family | File |
|------|--------|------|
| Display (h1, h2, wordmark) | Bricolage Grotesque | `fonts/bricolage-grotesque-latin-opsz.woff2` |
| Body / UI | Inter | `fonts/inter-latin-variable.woff2` |
| Code / kbd / mono | JetBrains Mono | `fonts/jetbrains-mono-latin-{400,500,600}.woff2` |

Bricolage uses `font-variation-settings: 'opsz' 72` at large sizes, `'opsz' 40` at small. Weight 800 for hero, 700 for headings, 600 for subheadings.

## Token vocabulary (`colors_and_type.css`)

### Surfaces (light → dark)
- `--background` / `--paper`: `#f6f4ef` → `#0b0b11`
- `--card` / `--raised`: `#ffffff` → `#12121a`
- `--sunken`: `#edeef3` → `#0a0a10`
- `--elevated`: `#eef1f7` → `#1a1a2e`
- `--hover`: `#e7e9f1` → `#1e1e2e`

### Text
- `--foreground` / `--ink`: `#1a1a22` → `#e0e0e0`
- `--muted`: `#5a5a6a` → `#888899`
- `--subtle`: `#69697e` → `#9a9bb0`

### Borders
- `--border` / `--line`: `#e2e2ea` → `#1e1e2e`
- `--line-soft`: `#ededf2` → `#16161f`

### Accent (indigo)
- `--accent`: `#3b5bdb` → `#8aa6ff`
- `--accent-hover`: `#2843c0` → `#a3bbff`
- `--accent-foreground`: `#ffffff` → `#0a0a10`
- `--accent-soft`: `rgba(59,91,219,.10)` → `rgba(138,166,255,.12)`

### Status
- `--success` / `--ok`: `#178035` → `#9ece6a`
- `--warning` / `--warn`: `#9a6b00` → `#e0af68`
- `--destructive` / `--err`: `#d11a2a` → `#f7768e`

### Syntax
- `--sx-kw` keyword, `--sx-str` string, `--sx-com` comment, `--sx-fn` function, `--sx-num` number, `--sx-type` type
- Also aliased as `--syntax-keyword`, `--syntax-string`, etc.

### Shadows
- `--win-shadow`: 4-stop window shadow with indigo undertone
- `--panel-shadow`: 2-stop flat lift

### Primitives
- `--font-display`, `--font-sans`, `--font-mono`
- `--radius-sm` 6px / `--radius-md` 10px / `--radius-lg` 14px / `--radius-pill` 999px
- `--space-1` (4px) through `--space-24` (96px)
- `--text-xs` (11px) through `--text-display` (56px)
- `--ease` / `--spring` / `--duration-fast/base/slow`

## Preview cards

All cards in `preview/` link `card.css` and hard-code current hex values. Critical cards: `semantic-colors`, `theme-light`, `theme-dark`, `syntax`, `type-display`, `brand-mark`.

## Usage

Link `colors_and_type.css` as the single stylesheet foundation. Card previews each link `card.css` (which embeds fonts and a minimal layout system).

Light is default (`:root`). Dark activates via `[data-theme='dark']` on `<html>`.
