# ADR-023: Markdown Writing Experience

**Status:** Accepted; `editor.markdown_typography` (decision 4) was removed on 2026-10-13 ([ADR-019](./019-open-anything-ladder.md)): nothing read it, and Inline | Source decides how Markdown is drawn. Amended 2026-10-15: blocks drawn in place, folding and a reading width (below)
**Date:** 2026-07-02

## Context

ADR-014 made the markdown buffer read well: heading scale, inline styles, and syntax-marker
hiding turned the editor into a live typographic surface. Writing in it still feels like
editing source. Three gaps separate it from the bar set by Obsidian's live preview and
Notion's editor:

1. **The parser sees CommonMark only.** `builtins.ts` registers `markdown()` on the default
   commonmark base, so GFM constructs — task lists, strikethrough, autolinks, tables — never
   enter the syntax tree. The typography plugin's `Strikethrough` branch is dead code in the
   app, and task lists cannot be decorated at all.

2. **No formatting commands.** Cmd+B, Cmd+I, Cmd+E, Cmd+K do nothing. Typing a marker over a
   selection replaces the selection. Toggling emphasis means hand-typing asterisks at both
   ends of a span.

3. **Structural marks render as source.** `- [ ]` stays six characters of punctuation,
   bullets stay hyphens, a thematic break stays three dashes.

List/quote continuation on Enter, empty-item termination, ordered-list renumbering,
Backspace markup deletion, and paste-URL-over-selection already ship inside
`@codemirror/lang-markdown` 6.5's `LanguageSupport` (high-precedence bundled keymap,
`pasteURLAsLink` default on) and need no new code — task-list continuation included, which
works off raw line text and re-emits continued checkboxes unchecked.

## Decision

### 1. GFM base

`register("markdown", () => markdown({ base: markdownLanguage }))`. One line; the tree gains
`Task`/`TaskMarker`, `Strikethrough`/`StrikethroughMark`, `Autolink`, and table nodes. All
downstream behavior keys off the tree, so this is the enabling move.

### 2. Editing layer: `src/editor/markdown-editing.ts`

A new extension, mounted through its own `Compartment` in `EditorInstance` and active only
when the buffer language is markdown and `editor.markdown_editing` (new config flag, default
true) is set:

- **Toggle commands** (`src/commands/markdown-format.ts`, pure `StateCommand`s):
  `toggleBold` (`**`), `toggleItalic` (`*`), `toggleStrikethrough` (`~~`), `toggleInlineCode`
  (`` ` ``), `insertLink`. Selection → wrap; selection exactly inside the matching node →
  unwrap (mark-token ranges deleted via `syntaxTree`); empty selection inside a word → wrap
  the word (`state.wordAt`); empty selection at a boundary → insert the pair with the cursor
  centered. `insertLink`: plain selection → `[sel](…)` with the cursor in the URL slot;
  selection that is itself a URL → `[…](url)` with the cursor in the label slot; empty →
  `[]()` in the label slot.
- **Keybindings:** Cmd+B, Cmd+I, Cmd+Shift+X, Cmd+E, Cmd+K — all verified unbound across the
  app keymap, CM defaultKeymap, preview keymap, and the native menu. Registered in the CM
  keymap for the keystroke and in the command registry (`scope: "editor"`) for the palette,
  the dual pattern `editor.addCursorUp` established.
- **Wrap-on-type:** an `EditorView.inputHandler` — the first in the codebase — that wraps a
  non-empty selection when `*`, `_`, `~`, or `` ` `` is typed, and falls through otherwise.
  `closeBrackets` was considered and rejected: its same-char path also auto-closes on empty
  selections, which is wrong for emphasis markers in prose.

Commands are pure functions over `EditorState` so the whole layer is unit-testable without a
DOM, matching the ADR-014 builder pattern.

### 3. Rendering layer: task checkboxes, bullets, thematic breaks

Extending the ADR-014 plugin, same reveal-on-active-line discipline:

- **Task checkboxes.** On inactive lines the `TaskMarker` range is replaced by a checkbox
  widget (`WidgetType`; `eq` on checked state). Clicks are handled by one
  `EditorView.domEventHandlers` mousedown at the plugin level — no per-widget listeners —
  which maps the event target back to a document offset via `view.posAtDOM` and dispatches
  `[ ]` ⇄ `[x]`. The document is the only state; the widget is a projection of it. On the
  active line the raw marker shows, as with every other mark. Atomic ranges are not needed:
  cursor entry makes the line active, which dissolves the widget back into source.
- **Bullets.** Inactive-line `ListMark` of bullet lists renders as a `•` widget; ordered-list
  marks keep their numbers with a muted mark class.
- **Thematic breaks.** An inactive `HorizontalRule` line's text is replaced by a full-width
  rule widget.
- **Autolinks** get the existing link text styling.

Tables stay source-rendered: column alignment in monospace is the honest editing view, and
half-rendered tables are worse than none.

All new CSS classes are `cm-md-*`, `.cm-editor`-scoped, colored exclusively by `--writ-*`
tokens, in `cm-markdown-typography.css`.

### 4. Config

`editor.markdown_editing: bool` (serde default true, missing-key tolerant — no migration)
gates the editing layer. `editor.markdown_typography` continues to gate the rendering layer,
checkbox interactivity included. Both flags get Settings rows (the typography flag had
plumbing but no UI) and `SETTINGS_INDEX` entries.

## Alternatives considered

**Gating the bundled continuation keymap behind the new flag.** Continuation has been live
since the markdown language was registered; pulling an already-shipped behavior under a new
flag regresses nobody's problem. The flag gates only the new layer.

**`closeBrackets` languageData for marker wrapping.** Rejected above — wrap and auto-close
are inseparable in its same-char path.

**Atomic ranges for the checkbox widget.** Rejected: the active-line reveal already
guarantees the cursor never coexists with the widget on a line, and atomic ranges would make
arrow motion skip real document characters on inactive lines.

## Consequences

- Task lists become first-class: parsed, rendered, clickable, and continued on Enter.
- A checkbox click is a normal document transaction — autosave, undo, FTS, and the preview
  all observe it with zero new plumbing.
- The editing layer is one compartment; plain-text purists set one flag and get today's
  editor back exactly.
- The GFM base slightly enlarges the parse work per edit; the parser is incremental and the
  decoration pass stays gated to `visibleRanges`, so the ADR-014 performance envelope holds.
- First `inputHandler` in the tree sets the pattern for future smart-typing behavior.

## Amendment, 2026-10-15: blocks drawn in place, folding, reading width

Decisions O-1 and O-4 of the v1 plan. Tables, display math (a paragraph that is one `$$…$$` span) and `mermaid` fences are drawn in place in Inline while the selection is outside the block, and show their source when the selection touches it, either end included; inline `$…$` follows the per-line marker rule. This supersedes "Tables stay source-rendered" in decision 3. Math comes from a grammar extension (`src/editor/markdown-math.ts`) that applies pulldown-cmark's `ENABLE_MATH` rules, so the editor and the preview agree on what is math. The widgets come from a state field over the viewport (a block widget may not come from a plugin), take whole lines, and hand CodeMirror the height last measured for their block hash while off screen. A block in a quote, or on a list item's first line, stays source: the widget would take the markers with it. KaTeX (`trust: false`) typesets in a worker. Mermaid (the preview's `mermaid.min.js`, `securityLevel: "strict"`, a theme per polarity) lays out on the page's thread, since it measures text in the document: one diagram per task, after typing pauses, cached by source. Both outputs go through a sanitizer over the parsed fragment before they are inserted into the editor. Mermaid draws into the document to lay out, so its `secure` list also locks `themeCSS`, `themeVariables` and `fontFamily` against a file's init directive, and the main window's CSP is what stops a request during that draw. It removes elements that run, load or submit, event handlers, links other than a `#` fragment or an inline raster, and every `<style>` element or attribute whose CSS, read with its escapes decoded, fetches: a `url()` other than `url(#…)`, `image-set()`, `@import`. The main window's CSP backs it. Raw HTML blocks stay styled source, being the author's markup rather than a renderer's. The preview's renderers are unchanged. Folds live in CodeMirror's fold state outside every compartment, so they survive Inline and Source and a tab switch. A heading folds from a chevron hung in the left margin (prose keeps ADR-030 decision 3's empty margin, so no gutter); a source file folds from a gutter; `Cmd-Alt-[` and `]` (`Ctrl-Shift-[` and `]` elsewhere, since Control-Alt is AltGr on Windows) fold at the cursor. `editor.reading_width` holds prose to `--writ-prose-measure`; code keeps the pane's width. No minimap.
