# 013 — Editor search controller

## Status

Accepted

## Context

The in-editor find experience (Cmd+F within the active document) used CodeMirror
6's stock search panel via an unmodified `searchKeymap`. That panel has no match
count, is not styled to the product, reflows the document when it opens, and is
not reachable through the app command registry. It reads as a default, not a
designed surface.

Two forces shape the replacement:

1. The find UI is a long-lived product surface and must meet the product bar:
   live `N of M` count, case / whole-word / regexp toggles, keyboard-first
   navigation, replace with capture references, and a scroll-track match map —
   all in a floating affordance that does not push the document down.
2. The editor engine is expected to change. CodeMirror is the current host; a
   native Rust editor is a plausible future. The find UI must not be welded to
   CodeMirror internals, or it is thrown away on that migration.

## Decision

Split the find feature into a **UI layer** and an **engine layer** joined by a
narrow interface, `EditorSearchController`.

```
FindOverlay (SolidJS)  ──uses──▶  EditorSearchController  ◀──implements──  CodeMirrorSearchController
```

`EditorSearchController` is the only contract the overlay knows. It exposes:

- `setQuery(query: SearchTerm): void` — push the current term + flags.
- `next() / previous(): void` — move the active match, wrapping.
- `replaceCurrent() / replaceAll(): void` — replace using the term's replacement,
  honouring capture references in regexp mode.
- `matchState(): MatchState` — `{ current, total, capped }` for the count badge.
- `matchPositions(): MatchTick[]` — fractional document offsets for the map.
- `clear(): void` — drop the query and its highlights.

The current implementation, `CodeMirrorSearchController`, is a thin adapter over
`@codemirror/search`, which is already a dependency — **no new third-party
package is added**. It reuses the library's matching, navigation, replacement
(including `$1` capture references) and match highlighting:

- Matching/highlighting is driven by dispatching `setSearchQuery`. The library's
  `searchHighlighter` view plugin decorates matches from search state regardless
  of whether the native panel is open, so highlights work with the panel closed.
- Navigation and replacement call the library commands `findNext`,
  `findPrevious`, `replaceNext`, `replaceAll`. These are wrapped in the library's
  `searchCommand`, which runs the command directly when a valid query already
  exists in state and only falls back to opening the native panel when none
  does. Because the controller always sets a valid query first, **the native
  panel is never opened**.
- The match **count** is the one thing the library does not expose. It is
  computed by iterating `getSearchQuery(state).getCursor(state)`, bounded by a
  cap (`MATCH_COUNT_CAP`) so a pathological query on a large buffer cannot freeze
  the UI; beyond the cap the badge shows `cap+`.

The native `searchKeymap` is removed from the editor configuration and replaced
by app-registry commands (`editor.find`, `editor.findNext`, `editor.findPrevious`,
`editor.replace`). The `search()` extension is still installed for its state and
highlighter; `highlightSelectionMatches` is retained (it is an unrelated
feature). The overlay is **non-modal**: the document stays interactive and the
global keymap continues to fire, so find-next works while editing.

## Consequences

- The overlay, its styling, the count, the toggles, and the scroll-track map are
  product code with no CodeMirror coupling. Migrating the engine means writing a
  second `EditorSearchController` implementation; the UI is untouched.
- Counting is `O(matches)` and recomputed on query and document change; the cap
  plus debounce bound the cost. Documents with more matches than the cap report
  `cap+`, which is acceptable for a count badge.
- Reusing the library commands means replace semantics (capture references,
  match advancement) match upstream behaviour exactly, with no reimplementation
  to keep in sync.
- A new control surface is introduced (the controller interface). New find
  behaviour is added to the interface and both layers, not bolted onto the
  overlay.

## Alternatives considered

- **Keep the stock panel, add a counter.** Rejected: the panel still reflows the
  document, cannot be themed without fighting library DOM, and leaves the UI
  welded to CodeMirror.
- **Override the library panel via `SearchConfig.createPanel`.** Rejected: panels
  rendered through that hook participate in CodeMirror's panel layout and reflow
  the document, which the no-reflow requirement forbids.
- **Fully custom engine (own cursor + decorations).** Rejected for now: it
  reimplements matching, highlighting and capture-aware replace that the existing
  dependency already provides correctly. The controller interface preserves the
  option to do this later (e.g. behind the Rust engine) without touching the UI.
