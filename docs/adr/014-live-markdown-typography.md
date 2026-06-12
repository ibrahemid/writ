# ADR-014: Live Markdown Typography in the Editor

**Status:** Accepted
**Date:** 2026-06-12

## Context

Markdown files are a primary writing surface in Writ — notes, prompts, documentation. Today the
editor applies heading weight (600) via the `writHighlight` token style and syntax colouring
via Lezer highlight tags, but the buffer reads like monospaced source text. The separate preview
pane renders rich HTML, but it requires the user to switch surfaces. For a tool positioned as a
writing instrument for prose and prompts, the writing surface itself should feel typographically
alive.

Two needs drive this:

1. **Reading rhythm while writing.** Scaled heading sizes, rendered bold/italic/strikethrough, and
   a visually distinct blockquote give the writer a document sense without leaving the editor.

2. **Honest editing.** Hiding syntax markers (#, **, _, `, [], ()) while the cursor is elsewhere
   and revealing them on the active line preserves the clean reading view without creating cursor
   traps or obscuring what is actually being typed.

## Decision

Implement live markdown typography as a CodeMirror 6 `ViewPlugin` + `DecorationSet`,
scoped strictly to markdown buffers, added to the editor extension stack via a `Compartment`.
All visual behaviour is disabled when the `editor.markdown_typography` config flag is false.

### Decoration strategy

**Line decorations** for heading scale: `ATXHeading1`–`ATXHeading6` receive `line` decorations
that inject `--md-heading-scale` CSS custom properties (1.6em → 1.0em). Line decorations do not
affect character positions, cursor motion, or selection so they are safe for all headings
regardless of cursor position.

**Mark decorations** for inline styles: `StrongEmphasis`, `Emphasis`, `Strikethrough`,
`InlineCode`, `Blockquote` (and inner content) receive `mark` decorations that add CSS class
names. These affect only rendered appearance, not document positions.

**Replace decorations** for syntax markers: `HeaderMark`, `EmphasisMark`, `CodeMark`,
`StrikethroughMark`, `QuoteMark`, `LinkMark`, and the URL span of `Link` are hidden via
`Decoration.replace({})` on lines that do not contain any cursor or selection endpoint. On the
active line all markers are revealed so editing is honest and predictable. This is the same
reveal-on-cursor pattern used by Typora and Bear.

`Decoration.replace` is chosen over opacity/color because it removes the marker from visual
flow without removing it from the document model. CodeMirror's cursor-motion logic never treats
replaced ranges as traps — the cursor still moves through the underlying characters; the replace
only affects what is painted.

### Visible range gate

All decoration building is scoped to `view.visibleRanges` only — never the full document.
This bounds work to O(rendered lines) regardless of document size and aligns with how CM6
itself gates painting.

### Theme integration

All colours and sizes use existing `--writ-*` CSS custom properties from `src/styles/theme.css`
and the token set in `cm-theme.ts`. No new hardcoded values are introduced. The CSS lives in
`cm-markdown-typography.css`, imported by `cm-theme.ts`. Because the token values flip via CSS
custom properties, all styles invert automatically between `warp-dark` and `warp-light` without
any polarity-specific logic in the plugin.

### Config flag

`editor.markdown_typography: bool` (default `true`) is added to `EditorConfig` in
`crates/writ-core/src/config/mod.rs` and to the `WritConfig` TypeScript interface. When `false`
the `Compartment` is reconfigured to an empty extension, restoring exactly today's behaviour.

## Alternatives considered

**Preview-only (status quo).** The preview pane already provides a rich view. The cost is a
surface switch: users writing notes or prompts must leave the editor to see a readable document.
This is fine for occasional review but creates friction for continuous writing. The preview epic
made the preview excellent; this makes the editor excellent for writing prose.

**Full WYSIWYG (replacing markers with rendered output in the CM document).** True WYSIWYG maps
CM document characters 1:1 to visible glyphs only through aggressive atomic decoration or by
replacing text ranges — both approaches create cursor traps where positions do not correspond to
document offsets, break selection extending, and are hostile to CodeMirror's `search` extension
(which computes match positions over the raw document). Replace decorations on inactive markers
is a well-understood middle path: cursor motion is unaffected because replaced ranges are
traversable, and `@codemirror/search`'s cursor operates on state.doc which is never mutated.

**Plain CSS overrides for `.cm-line` descendants.** Possible for font weight / italic but cannot
produce heading scale without line decorations (the required `font-size` escalation affects line
height which CM must know at decoration time), and cannot implement marker hiding without access
to the syntax tree. Not viable for the full feature set.

## Consequences

**Positive:**
- The writing surface conveys document structure without switching panes.
- Marker hiding reduces visual noise; active-line reveal keeps editing honest.
- Pure decoration model: no document mutations, undo history untouched.
- Replace decorations on inactive markers do not affect `@codemirror/search` match positions
  (cursors iterate over `state.doc`, not the decorated view).
- Feature is invisible when the config flag is off — zero regression surface for users who
  prefer plain-text mode.
- Decoration builder is a pure function; unit-testable without a DOM.

**Negative / risks:**
- Replace decorations and mark decorations must be rebuilt on every view update that changes
  `visibleRanges` or the selection. This is bounded to visible lines and is the same work
  pattern used by `highlightSelectionMatches`; profiling should confirm it stays under 1 ms on
  typical documents.
- `Decoration.replace` on marker characters requires that the replaced ranges never overlap.
  The builder must guard against this — the tests cover the overlap-free invariant.
- Heading scale line decorations change line height. CodeMirror handles this correctly via its
  block height cache, but any third-party extension that assumes uniform line height may behave
  unexpectedly. No such extension is currently in use.
- The plugin requires the Lezer markdown tree to be fully parsed for visible lines. For very
  large documents the tree parser may not have finished, in which case `syntaxTree` returns a
  partial tree. The builder skips any range where the tree cursor returns no node, so it
  degrades gracefully to no decoration rather than crashing.
