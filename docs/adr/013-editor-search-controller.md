# 013 — Editor search controller

## Status

Accepted; amended 2026-10-13 (the count and the match map come from one scan in slices, matches are highlighted by Writ's own highlighter, an invalid pattern says so)

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

### Amended: one scan gives the count and the match map

Counting by walking `getCursor` held the main thread for every match in the
document, on every query change and on every edit while the overlay was open:
547 ms per keystroke in the find field on a 4.9 MiB file, 4 s on 50 MiB (the
editor-perf audit, 2026-10-03). The count and the match map now come from one
pass, kept by a view plugin (`src/editor/search/find-matches.ts`). A regular
expression that can cross lines is counted by its own matches over the whole
text, while a match and its lookarounds stay within 4 KiB of where it starts
(the windows below), and that count can differ from the matches
`SearchCursor` steps through:

- The matcher (`src/editor/search/scanner.ts`) finds what
  `SearchQuery.getCursor` finds, except as the next two points say. A literal
  query is folded per code point the way `SearchCursor` folds (NFKD, then
  lower case when the query ignores case), with a native search on text
  where no code point past ASCII can be part of a match (known per node of
  the document), and the whole-word test uses the editor's word characters. A
  regular expression is compiled once with the library's flags and picks up
  after each match where the library's cursors do: `RegExpCursor` line by
  line, or, for a pattern the library reads as able to cross lines (one
  holding `\s`, `\W`, `\D`, `\n`, `\r` or `[^`), `MultilineRegExpCursor`
  across them, down to its stop at the first match the whole-word test turns
  down.
- A literal query, a line over 64 KiB, and a query that crosses lines are
  read in windows of 64 KiB. For a regular expression a window also reaches
  4 KiB behind the search, and a match is taken only when it ends 4 KiB
  before its window does
  or the window holds the rest of the line or document; a match that keeps
  going from where the search stands grows its window until it ends, in one
  task. A literal finds exactly the library's matches. A regular expression
  finds its own matches over the whole text whenever they and its lookarounds
  stay within 4 KiB of where they start. One that looks further can end a
  match early: `a.*b` reaching for the last `b` of a line over 64 KiB, or a
  pattern that crosses lines, such as `a\s.*b`, on any line over 4 KiB that
  crosses a window edge, since its windows run over the whole document.
- `MultilineRegExpCursor` reads from where it starts in a chunk ending at the
  end of the line holding the position 5,000 characters on, doubles the chunk
  when it finds no match it can take, and takes a match only when it ends 10
  characters before the chunk does. A greedy run that can cross lines
  (`[^z]*`, `[\s\S]*`, `\D*`) followed by a terminator that repeats ends at
  the chunk's end there, where Find reads on: `a[^z]*b` over `a b`, a line
  break and `b a b z` is one match for Find and two for the cursor when its
  chunk ends at that line break. On the 8,000-line generated log of the tests,
  `e[^z]*t` counts 8,009 matches against the cursor's 8,017
  (`@codemirror/search` 6.7.2). The cursor's matches depend on where it
  starts, and the count reads from the top, Enter from the selection and the
  highlighter from the visible ranges, so no single cursor answer exists for
  the count to copy, and the count keeps the expression's matches over the
  whole text. Where Enter selects a match the count does not hold, the badge
  shows the count without the match's place.
  `src/__tests__/editor/find-matches-parity.test.ts` holds the matcher to the
  library's cursor over fixtures, random documents and random edits, and to
  the expression over the whole text for such greedy patterns on generated
  documents; `find-matches-windows.test.ts` holds it to the cursor with
  windows a few dozen characters wide, so that window edges, reads cut short
  and edits all cross.
- Up to 1 MiB the document is read in the update that set the query. A longer
  one is read in slices of 6 ms, each in its own task, the deadline checked at
  least every 64 KiB; until the last slice the badge shows the running count
  with a `+` and is `aria-busy`.
- An edit is read again from a point before it where the reading stood
  between matches, until the reading stands where it stood before the edit;
  from there the matches read before the edit move with it. For a query whose
  matches stay on one line that is the lines the edit touched; inside a line
  over 64 KiB, and for a query that crosses lines, it is 4 KiB either side
  of the edit (for a literal, twice its length and 256 characters). The update
  that made the edit reads up to 128 KiB again; the slices read any rest.
- The cap stays `MATCH_COUNT_CAP` (10,000): the reading stops right after the
  last match it counts, and an edit past that point reads nothing again.
- The find field applies the query 80 ms after the last keystroke
  (`FIND_INPUT_DEBOUNCE_MS`). Enter, Shift+Enter and replace apply a pending
  query at once; closing the overlay drops it. The badge describes the query
  last applied: while a typed one waits, it keeps the last count and is
  `aria-busy`, and it says nothing until a first query has been applied.
- A tab switched to while the overlay is open gets the query, so the count and
  the highlights are the file's in front. When the find target changes with
  the overlay closed, the surface now in front is cleared: Find closed in
  preview-only leaves no query, and so no highlights, in the editor it left.

### Amended: Writ highlights the matches

The decision above is wrong about highlighting: the library's
`searchHighlighter` decorates matches only while its own panel is open, and the
panel is never open, so no match was highlighted. The plugin decorates the
visible ranges itself, with the library's classes (`cm-searchMatch`,
`cm-searchMatch-selected`) and so its theme, from `getCursor` over each visible
range and the same margins the library reads. Visible ranges whose margins
would overlap are searched as one, and each search keeps only the matches that
start before its margin ends: for a pattern that crosses lines the library's
cursor reads on to the end of the line holding its start + 5,000, which on a
long line shown without wrapping holds the next visible range's matches, and
marks added out of order stop the highlighter. It stands down if the native
panel is ever open. A field holding only whitespace is no query, as the badge
and Enter have always treated it, so it highlights nothing.

### Amended: an invalid pattern says so

`searchCommand` opens the native panel when the query is not valid, so Enter on
a regular expression that does not compile opened it. The controller checks
`query.valid` before every library command; an invalid query answers
`invalid: true` from `matchState()`, the badge reads "Invalid pattern", and
nothing moves. A query with no matches skips the commands too, which would
otherwise walk the whole document to find nothing.

A match attempt can also outgrow the regular expression engine. V8 then
throws a `RangeError`, as for `[\s\S]*` over 8 MiB of text holding a
character outside Latin-1 (measured under Node 22 and 26 and Chromium 153;
WebView2 runs V8), and the library's cursor throws on the same input. The
reading stops there and `matchState()` answers `failed: true`: the badge reads
"Pattern too complex", the match map stays empty and nothing moves; after an
edit it says so until the next reading settles. The highlighter keeps what it
painted before the engine gave up. JavaScriptCore (the macOS webview) answers
such an attempt with no match instead, and the reading goes on past it:
`(\D)+` over 2 MiB of text without digits counts one match, the text's last
8 KiB, where the library's cursor counts none and the expression has one over
the whole text.

Moving to a match and Replace All still run the library's cursor from the
selection. On a large file with one match far from the caret, Enter walks the
document between them in one task.

The budgets for one find keystroke and one edit with the overlay open (a word
and patterns that cross lines, on 4.9 MiB to 20 MiB, and a 5 MiB line), and
the browser-measured ones per file tier, are in `docs/perf-budgets.md`. The
Find budgets there stand on measurements of this scan, without the editor-perf
audit's figures as a floor, since the audit measured the count this scan
replaced; every other budget there is at least the audit's figure for its
file.

## Consequences

- The overlay, its styling, the count, the toggles, and the scroll-track map are
  product code with no CodeMirror coupling. Migrating the engine means writing a
  second `EditorSearchController` implementation; the UI is untouched.
- Counting reads the document once per query, in slices, and after an edit
  only around the edit; the cap bounds the matches held. Documents with more
  matches than the cap report `cap+`, which is acceptable for a count badge.
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
