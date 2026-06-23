# ADR-021: vfinal Site Rebuild

**Status:** Proposed
**Date:** 2026-06-23

## Context

The marketing site at `site/` is the storefront: it carries the entire first
impression of Writ. It currently ships the old design and three things that
must not survive this rebuild:

1. `framer-motion` (banned; the house standard is `motion/react`).
2. Invented install commands in `Install.astro` (`brew install --cask
   ibrahemid/writ/writ`, `winget install --id ibrahemid.Writ`, a `curl |
   sh` one-liner). None of these distribution channels exist.
3. A React `HeroDemo.tsx` that bears no relation to the approved design.

The approved design is `site/.mockups/vfinal/index.html`, locked on 2026-06-18
(`.status/writ-vfinal-final-audit-2026-06-18.md`). It is design-final and is
not to be redesigned. 0% of it is implemented in the real site today; this is a
full rebuild, not a refactor.

The mockup is a single self-contained HTML file. It loads fonts from Google
Fonts, KaTeX CSS from jsDelivr, and `marked` / KaTeX / Mermaid from CDNs, and it
hand-rolls a scroll-linked deck animation with a `requestAnimationFrame` lerp
loop. Every one of those is a placeholder the mockup's own comments flag for
Phase 3 to replace (`<!-- live render libs (mockup only; Phase 3 bundles these
locally) -->`).

The governing constraint for this epic, inherited from the audit, is absolute:
**no claim ships on the live site unless it is true in the built app.** That
single rule is what forces the hardest decision below (markdown rendering) and
is why this ADR rejects the cheap answer there.

The site deploys to GitHub Pages under base path `/writ`
(`site/astro.config.mjs`). Every asset URL is subject to that subpath.

## Decision drivers

- The storefront is judged on craft, not on bundle weight. We do not optimize
  for less work. The live editor and the scroll deck are the two surfaces a
  visitor evaluates us on; they must be genuinely high-craft, not a mechanical
  port of the mockup's throwaway JS.
- No claim renders that is not true in the shipped app. A lookalike renderer is
  a false claim.
- Zero JS islands unless unavoidable. Every island is justified here in writing.
- No CDN. Fonts and render libraries are self-hosted / bundled.
- One source of truth for design tokens. No hand-copied hex.
- AA contrast, `prefers-reduced-motion` honored, keyboard-navigable, 390px
  responsive, clean `pnpm build` and `tsc`.
- Asset URLs must survive the `/writ` base path in production.

---

## Decision 1: Islands framework — React + `motion/react`

### Considered options

**A. Vanilla-TS Astro islands (no UI framework).**
Closest to the mockup's existing vanilla JS; smallest possible runtime.
Rejected: this is the cheap path chosen for bundle size, which is the wrong
basis for the storefront. It also means hand-rolling the scroll-deck physics and
reduced-motion fallback that a real motion library gives us for free, which
works directly against the "high craft, not a mechanical port" requirement for
the deck.

**B. SolidJS islands (`@astrojs/solid-js`), matching the app.**
The app is SolidJS, so this argues for stack parity. Rejected on merit, not
effort: the site shares no components with the app and never will (different
content, different lifecycle, a marketing demo rather than the product). The
real source of app parity in this rebuild is the renderer (Decision 2) and the
bundled render libs, not the view framework. Meanwhile Solid's scroll-linked
animation story (`solid-motionone`) is materially less mature than
`motion/react` for the orchestrated, spring-damped, scroll-driven morph the deck
requires. Choosing Solid would buy a parity that delivers nothing here and pay
for it in the exact surface we are judged on.

**C. React + `motion/react`.** (chosen)
`motion/react` is the documented house standard for React motion work, and the
project rule names this case explicitly: "Anything involving stagger,
scroll-linked transforms, parallax, orchestrated reveals, or spring physics uses
`motion/react`." The deck is precisely scroll-linked transforms plus spring
physics. `useScroll` + `useTransform` + `useSpring` + `useReducedMotion` are
purpose-built for it and give us a higher-craft, reduced-motion-correct deck than
the mockup's hand-tuned lerp. CodeMirror 6 is framework-agnostic, so React hosts
the live editor without friction.

### Decision

Use React 18 + `motion/react` for the islands. Remove `framer-motion`,
`react`/`react-dom` stay only as `motion/react` and CodeMirror hosting
dependencies. Replace `HeroDemo.tsx` entirely.

`framer-motion` is uninstalled; `motion/react` replaces it. The
`vite.ssr.noExternal: ['framer-motion']` shim in `astro.config.mjs` is removed.

### Islands (each justified, per the zero-island rule)

1. **Live editor** (`LiveEditor`): CodeMirror 6 instance + the real Writ
   markdown renderer (Decision 2) + bundled KaTeX/Mermaid. Reactive by nature;
   cannot be static.
2. **Scroll deck** (`Deck`): scroll-linked morph of one persistent window.
   Inherently scroll-reactive; `motion/react` drives it.
3. **Theme toggle** (`ThemeToggle`): light/dark switch with no-flash inline
   bootstrap. Minimal, isolated.
4. **Status island** (`StatusIsland`): the dynamic-island status pill that
   cross-dissolves through live states on a timer, hover-expands to the full
   list, and tucks away once the hero scrolls past. Part of the locked design
   and genuinely reactive (timed state machine + hover + scroll). Minimal,
   `client:idle`, reduced-motion shows a single static state.
5. **Download OS detection** (`DownloadPicker`): reorders the download surface to
   lead with the visitor's platform (Decision 5). Pure progressive enhancement:
   the static markup ships all three platforms first-class; this only promotes
   one. Smallest possible client footprint (a `client:idle` reorder), not a
   render dependency.

Everything else (header, hero static cluster, comparison table, footer) is
static Astro with zero client JS. The download surface renders all three
platforms statically and is fully usable with JS off; the picker only
reorders. Five islands is more than the zero-island ideal, but each is a
reactive surface in the signed-off design and none is removable without
dropping designed behavior; the two heavy ones (live editor, deck) carry the
React + `motion/react` runtime, the other three (`ThemeToggle`, `StatusIsland`,
`DownloadPicker`) are lightweight `client:idle` enhancements.

---

## Decision 2: Markdown rendering — run the real renderer via WASM

This is the hard decision, and the one the storefront promise turns on.

The app renders markdown with Rust `pulldown-cmark` 0.12 in
`src-tauri/src/preview/renderers/markdown.rs`, with a specific option set
(`ENABLE_TABLES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS | ENABLE_FOOTNOTES |
ENABLE_MATH`), a custom event-stream pass that rewrites ` ```mermaid ` fences
into `<pre class="mermaid">` blocks, and code-aware `$…$` math tokenization.
KaTeX and Mermaid are bundled JS libraries (`src-tauri/assets/katex/`,
`assets/mermaid/`) the renderer injects by reference.

### Considered options

**A. `marked` (JS) with reworded on-page copy so we never claim parity.**
Rejected, explicitly. This was the first instinct and it is backwards. The
product promise is "no claim ships unless true in the built app." Choosing a
lookalike parser and then softening the copy to dodge the gap is the exact
compromise we are refusing. `marked` and `pulldown-cmark` diverge in real,
visible ways for the GFM features agents emit (footnote rendering, task-list
markup, table edge cases, and especially the math tokenization, where
`pulldown-cmark`'s `ENABLE_MATH` keeps a multi-line `$$` block intact while
`marked` has no equivalent and needs a plugin that splits differently). The
demo would render agent output subtly unlike the app while sitting under copy
that says "this is what Writ shows you."

**B. Compile `pulldown-cmark` to WASM, generic config.**
Closer, but still wrong: a stock `pulldown-cmark` build does not reproduce
Writ's option set or the mermaid-fence rewrite or the math passthrough. It would
be a second, parallel implementation that drifts from the app the moment either
side changes.

**C. Extract Writ's actual markdown-fragment logic into a pure leaf crate and
compile that to WASM; the site and the app both call it.** (chosen)

The renderers (`markdown`, `katex`, `mermaid`, `theme`) in
`src-tauri/src/preview/renderers/` import **zero `tauri`** — only
`writ_core::preview` types, `pulldown-cmark`, and each other. Only `handler.rs`
(the protocol adapter) touches `tauri`. The fragment-producing core (the event
walk + fence rewrite + `html::push_html`) is already pure Rust sitting in the
wrong crate. Per the Rust boundary rule ("if a function doesn't need Tauri, it
doesn't belong in `src-tauri`"), it arguably should already have been extracted.

So: introduce a pure leaf crate `crates/writ-render/` whose only dependency is
`pulldown-cmark`. It exposes one function that takes markdown text and returns a
fragment (`{ html, has_mermaid, has_math }`) using Writ's exact options and
fence/math handling. `src-tauri`'s `MarkdownRenderer` is refactored to call it
and then do its app-side wrapping (theme document, `writ-preview://` runtime
injection); behavior is unchanged and the existing preview tests prove it. The
site compiles `writ-render` to `wasm32-unknown-unknown` via `wasm-bindgen`,
loads the wasm in the `LiveEditor` island, and wraps the same fragment in the
site's own chrome with its own locally-bundled KaTeX/Mermaid runtimes.

Result: the site editor runs the **identical** markdown parser, option set, and
mermaid/math handling as the shipped app, because it is the same compiled code.
The claim is true by construction.

### Why this is correct on the numbers, not just on principle

- **Bundle (measured, Phase B):** the `wasm-bindgen` wrapper over
  `pulldown-cmark` alone (the leaf crate carries no `getrandom`/`chrono`/`uuid`,
  which is exactly why it is a leaf crate and not "compile all of `writ-core`")
  measures **242 KB raw / 98 KB gzip** after `wasm-opt -Oz`. The initial
  estimate of ~150 KB / ~60 KB was low: `pulldown-cmark`'s Unicode
  case-folding/segmentation tables dominate and do not shrink under `-Oz`. This
  overage is **accepted**, because the wasm module is **not the bundle
  bottleneck**: it is lazy-loaded on editor visibility/interaction (never on
  first paint), and it sits behind Mermaid (~500 KB) and KaTeX (~280 KB +
  fonts) which are bundled identically under any option. 98 KB gzip is the price
  of true parity; the rejected `marked` lookalike (~40 KB) would be lighter but
  is a correctness lie. Budget is therefore revised to **≤ 260 KB raw /
  ≤ 110 KB gzip** for the renderer wasm, with the lazy-load requirement as the
  real perf control.
- **Build tooling note (Phase B):** `wasm-pack`'s bundled `wasm-opt` is too old
  for the bulk-memory opcodes rustc emits; the build disables it
  (`wasm-opt = false` in crate metadata) and runs a system `wasm-opt` instead.
  The Pages deploy (`site.yml`, Phase H) must install `wasm-opt` on PATH.
- **Build coupling:** the site build gains a wasm step (`wasm-pack` /
  `cargo build --target wasm32-unknown-unknown` + `wasm-bindgen`). This couples
  the site build to the Rust toolchain. That cost is bounded: the repo is
  already a Rust workspace, every contributor and CI runner already has the
  toolchain, and the GitHub Pages deploy workflow can run the wasm build as a
  prebuild step. We do **not** commit the wasm binary to git (it would bloat the
  repo and violate the repo-cleanliness rules); it is a build artifact.
- **Architecture:** the extraction is a net improvement to the app, not a tax on
  it. It moves pure render policy out of the Tauri crate into a pure crate where
  the boundary rule says it belongs, and it makes the renderer independently
  testable without a Tauri harness.

### Decision

Create `crates/writ-render/` (pure, `pulldown-cmark` only). Move the
markdown-fragment core into it. Refactor `src-tauri` `MarkdownRenderer` to
delegate (no behavior change, existing tests green). Compile `writ-render` to
WASM and run it in the site's `LiveEditor`. KaTeX and Mermaid are the same
bundled JS the app ships, vendored locally via npm and lazy-loaded on editor
visibility/interaction (never blocking first paint).

**This expands the epic's blast radius beyond `site/` into the Rust workspace
and the deploy pipeline. That is the one consequence flagged for explicit
sign-off (see Open questions).**

---

## Decision 3: Token pipeline — extend the JSON themes, emit CSS at build time

### Considered options

**A. Hand-author `tokens.css`.** Rejected: violates "one source of truth, no
hand-copied hex," and the mockup already drifted from the JSON on `--subtle`.

**B. Astro/Vite integration generating tokens at build.** Heavier than needed
for two static JSON files.

**C. A prebuild Node script reading the theme JSON and emitting `tokens.css`
(light + dark).** (chosen) Simplest correct mechanism; one source, regenerated
on every build, with a test asserting the emitted CSS matches the JSON.

### Decision

A prebuild step reads `src/styles/themes/warp-light.json` and `warp-dark.json`
and emits the site's CSS custom properties for both polarities.

**The JSON is not a 1:1 superset of the mockup tokens, and resolving that gap is
part of this decision, not an afterthought:**

- The JSON lacks site-only tokens the mockup needs: `--paper`, `--paper2`,
  `--seam`, `--win-shadow`, `--panel-shadow`, the `--traffic-*` lights, and the
  `--ease`/`--spring` easings. These are **extended into both theme JSON files**
  so the pipeline stays single-source. (These tokens are site-presentational; if
  the app ever wants them they already live in the shared theme JSON.)
- The one genuine conflict: `--subtle` is `#69697e` in the approved mockup vs
  `#76768a` in `warp-light.json`. Audit 2 flagged this exact color as the
  borderline-AA label color. **The winner is chosen by measured AA contrast
  against `--paper` (#f6f4ef), not by assuming the JSON is authoritative.** The
  measured value and the resulting AA pass/fail are recorded in this ADR before
  the pipeline ships. If neither hits 4.5:1 at the 9.5px label size, the label
  color is darkened until it does and the JSON updated to match.

---

## Decision 4: Fonts — self-hosted woff2, base-path-safe

Self-host Bricolage Grotesque (variable, with the `opsz` axis the mockup uses),
Inter, and JetBrains Mono as woff2. `@font-face` + `<link rel="preload">` for
the above-the-fold faces. Drop the Google Fonts and jsDelivr CDN links entirely.

Every font URL, preload, favicon, and OG asset path is routed through Astro's
base-aware handling (`import.meta.env.BASE_URL` / Astro asset resolution), never
an absolute `/fonts/...` root, so they resolve under the `/writ` subpath in
production. This is the classic "builds clean, 404s in production" trap and is
called out as a hard requirement, not a detail.

---

## Decision 5: Honest launch wiring — three first-class platforms at launch

Writ ships **all three platforms at launch**: macOS, Windows, and Linux. The
"mac-first, Linux beta, Windows later" language from the mockup is dropped
entirely. The release matrix (`.github/workflows/release.yml`) already builds
and uploads, validated:

- **macOS** universal: `Writ_<ver>_universal.dmg` and `Writ_<ver>_universal.pkg`
- **Windows** x86_64: `Writ_<ver>_x64_en-US.msi`
- **Linux** x86_64: `.deb` and `.AppImage`
- Checksums: `SHA256SUMS.txt`; updater manifest: `latest.json`.

### Download model: tri-platform parity, OS-led

The mac-dock metaphor in the locked mockup cannot honestly carry three
platforms (it is a macOS Dock; putting Windows/Linux into it would be a lie of
metaphor). The dock is therefore **evolved, not discarded**: it remains the
craft centerpiece for the detected-macOS case, but the download section is built
as a tri-platform surface where each platform is a first-class card with its own
icon, artifact, version, and checksum. OS detection (`DownloadPicker` island)
**leads with the visitor's platform**; the other two are clearly present and
one click away, never hidden and never labelled "coming soon." With JS off, all
three render equally. The exact visual evolution of the dock-into-tri-platform
surface is an implementation-phase craft decision; the ADR-level requirement is
**parity of presentation** across the three.

### Release gating

`site/src/data/release.json` holds placeholders today (all `"#"`). It is
extended to carry, per platform, the artifact URL, version, and SHA-256
checksum (sourced from the release's `SHA256SUMS.txt`). Until the v0.1.0 tag
publishes real assets, every platform card renders disabled / "available at
launch"; no live link points at a placeholder. When the release ships, the data
file is populated and the cards go live. This is one gate covering all three
platforms.

### Notarization trust line

The "signed & notarized · open source" trust line is **designed in but gated
off**. The Apple Developer account is being provisioned before launch, so
notarization will be real by ship, but the line stays omitted until a notarized
build actually exists (the release workflow already conditions signing on
`APPLE_*` secrets). The markup and styling for the trust line ship dormant and
switch on via the same release-data gate; we do not claim signing/notarization
early.

### Claims reconciliation

Every claim reconciled against the shipped app per the audit. The vfinal mockup
already bakes most fixes (island says "no account · no telemetry"; cold-start
says "target <200 ms"; deck scene 1 uses `writ ~/agent-out`, not the
non-existent `--watch`). This rebuild preserves the honest wording and does not
reintroduce "0 network calls" / "no network" as an unscoped app claim. The
deck's scene-5 "No network" is scoped to the preview origin
(`CSP default-src 'none'`), which is true, and stays scoped. The comparison
table's platform-relevant rows are reconciled to three real, shipping platforms
(no "beta"/"later" qualifiers anywhere on the page).

---

## Decision 6: Page structure

The mockup is single-page. The existing site has `index`, `download`,
`changelog`, `privacy`, `404`, and the footer links Changelog / Privacy /
GitHub.

Decision: `index` becomes the vfinal single page, including the tri-platform
download surface at the `#get` anchor (Decision 5). Keep `privacy`, `changelog`,
and `404` as real routes (footer links to the first two). Fold the standalone
`download` route into the `#get` anchor and redirect `/download` there so
existing links do not break.

---

## Consequences

**Callers / app:** introduces `crates/writ-render/`; refactors `src-tauri`
`MarkdownRenderer` to delegate to it. No app behavior change; existing
`src-tauri/src/preview/renderers/markdown.rs` tests must stay green and are the
proof. New crate gets its own unit tests (boundary rule: new public Rust
functions require tests).

**Packaging / build:** `framer-motion` removed. Site build gains a WASM
prebuild step and therefore a Rust-toolchain dependency **in the GitHub Pages
deploy workflow only**. This step is NOT duplicated into the app's release
matrix. The release matrix (`release.yml`) and CI matrix (`ci.yml`) fire only on
`main` push / PR-to-main / `v*` tags; nothing in this epic adds a trigger on
`dev` or `feat/*`, so the 10x-billing mac/Windows runners are never touched by
site work. The Pages deploy stays lean: wasm build + Astro build, single
runner. WASM artifact is build-output, not committed. Fonts and render libs
vendored locally; no CDN at runtime.

**Tests:** token-pipeline output test (emitted CSS matches JSON); markdown
parity is covered by reusing the app's own renderer tests against the extracted
crate; `writ-render` unit tests; `pnpm build` + `astro check`/`tsc` gates.

**Security:** no CDN (no third-party origin, no SRI gap). Site CSP stays clean.
All asset paths base-path-safe. The site editor's rendered output is a local
demo; it makes no outbound calls.

**Performance budgets (measured before merge):**
- `writ-render` wasm: target ≤ 150 KB raw / ≤ 60 KB gzip. Measured value
  recorded here before merge.
- Mermaid/KaTeX: lazy-loaded on editor visible/interaction; must not block first
  paint or LCP.
- Deck: 60fps scroll on the reference machine; static stacked fallback under
  `prefers-reduced-motion` and ≤880px.

## Open questions deferred to follow-up

1. **(Needs operator sign-off, not a later ADR.)** Decision 2 expands this epic
   into the Rust workspace (new crate + `src-tauri` refactor) and into the
   deploy pipeline (Rust toolchain in the Pages workflow). This is the right
   architecture and the only way to honor "no claim unless true," but it is a
   scope expansion beyond `site/` and is called out explicitly rather than
   buried.
2. WASM build mechanism in CI (wasm-pack vs raw `cargo` + `wasm-bindgen`, and
   caching) is an implementation detail settled in the first implementation
   phase, not a fork.
3. Linux/Windows download rows activate when those signed builds exist.
4. Whether the extracted `writ-render` later absorbs the HTML renderer too
   (so the site could demo `.html` rendering with real parity) is out of scope
   here and a candidate follow-up.
