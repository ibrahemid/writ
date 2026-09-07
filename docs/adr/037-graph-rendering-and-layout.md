# ADR-037: Where a graph is worked out and what draws it

## Status

Accepted.

## Context

Two surfaces draw the folder's link graph: "Connections", the panel beside the
note, which shows the open note and everything one link away from it, and
"Graph", the whole-folder view that follows it, opened from the command palette
as "Open graph". Both take their rows from the read model in ADR-036, and both
have to turn those rows into coordinates and then into pixels. Those are the
names the app uses: the panel's section reads "Connections", the whole-folder
view's heading reads "Graph", and the palette opens it as "Open graph".

That is three separate calls, and each one has a wrong answer that costs
something later:

- which side of the Tauri bridge works out where a note sits;
- what draws the result;
- whether a redraw of the same folder produces the same picture.

The third is not a nicety. A drawing is the one surface with no text to check
it against: if the same folder settles into a different shape on every open,
nobody can tell a layout change from a folder change, and no test can hold a
line under either.

## Decision

### 1. The rows are Rust, the geometry is TypeScript

`NotesIndex::graph` is the only source of nodes and edges (ADR-036). What links
to what is policy, it is derived from the link resolver, and it stays in the
crate that owns it.

Where a note sits is not policy. It is view geometry, it depends on the size of
the element it is drawn into, and it is recomputed on every frame of a settle.
Running it in Rust means one bridge round trip per tick, with the whole
position set serialised each way, for a loop that runs sixty times a second.
The only way to avoid the round trip is a second wasm artifact.

Writ already compiles a crate to wasm: `writ-render` carries
`crate-type = ["lib", "cdylib"]` and its own wasm-pack and wasm-opt pipeline
because the website renders Markdown with the same code the app does. That
pipeline pays for itself because the crate has two consumers. The layout has
one. A wasm crate for it would buy a build step, a `wasm32` target in CI and a
second artifact to ship, in exchange for arithmetic that is a few hundred
floating-point operations per frame.

So `src/lib/graph/layout.ts` owns the geometry, and the workspace boundary is
not crossed by it: nothing about which notes are linked is decided there.

### 2. The settle is seeded and deterministic, and that is a test

`layout.ts` opens no file, reads no clock and never calls `Math.random`.
Randomness comes from a linear congruential generator in the same file, seeded
by the caller; the panel seeds it from the open note's path, so one note
settles into the same shape on every visit.

Determinism has to hold across machines, not only across two runs on one, so a
position is built from `+ - * /` and `Math.sqrt` alone. Those are exact under
IEEE-754 on every engine. `Math.pow`, `Math.hypot` and the trigonometric
functions are left to the implementation and differ in the last bits between
platforms, so none of them takes part: the starting positions are drawn from
the generator inside a rectangle rather than placed around a circle.

The tests hold the property directly. The same rows and the same seed serialise
byte for byte, in one process and in a second `node` process spawned by the
test, which is the check that survives a change of engine.

### 3. The settle is in world units and the canvas fits it, so the minimum separation holds at any size

Every pair pushes apart, every link pulls together, the origin pulls everything
in, and a positional pass afterwards holds every pair at a minimum separation
so two notes cannot end up as one dot.

Every note starts on an even lattice about a link apart, nudged off its point
by a quarter of that. A folder's worth of notes dropped into one small square
instead starts with notes on top of each other, and repulsion goes as one over
the distance cubed: two thousand notes started that way threw each other out to
a span of ten million units and never came back. Starting them at about the
spacing they settle at costs nothing at ten notes and is what makes two
thousand settle at all.

None of that happens inside the canvas rectangle. The settle works in unbounded
coordinates and `fitToView` scales and shifts the result into whatever room the
canvas has, one scale for both axes so nothing is squashed. That split is what
makes the minimum separation a property of the settle rather than of the panel:
there is no area to run out of, so a hub note with four neighbours and a folder
where two hundred notes all link to each other both come back with every pair
at least the minimum apart. The tests hold a star and a clique to it at eight,
twelve, sixty-four and two hundred notes, and again at four hundred, which is
the size where the forces alone stop being enough and the pass that pushes
notes apart is the only thing doing it.

A note's springs are shared out over its links rather than summed. Two hundred
springs pulling one note inward every step is a pile no separation pass can
undo; sharing them out lets the same folder settle into a packing. That pass
also bounds the arithmetic: repulsion goes as one over the distance cubed, and
a step that ends with no pair closer than the minimum is a step whose successor
cannot work out an infinite force.

The minimum is satisfied by making room first and pushing pairs apart second.
The forces settle a drawing at whatever density they balance at, and past a few
hundred notes that density has no room for the minimum at all: the pass would
push pairs apart for ever, each push crowding another pair, and stop at its cap
with notes still touching. So before the pass runs, the whole drawing is opened
out until it holds a square the minimum across per note with a fifth to spare.
That is one scale about its own centre, which says nothing about where any note
sits relative to another. Past a couple of hundred notes the pass then finds its
pairs through a grid of cells the minimum across rather than by walking every
pair: two notes too close are in the same cell or a touching one, so it is the
same answer for a fraction of the work.

A whole folder settles for 120 steps at a minimum separation of twelve units
(`FOLDER_LAYOUT_OPTIONS`), against the panel's 240 steps at twenty-two: the
notes are smaller and there are more of them, and a longer settle buys a folder
nothing a reader can see. Measured on an M-series laptop, two thousand notes
settle in about 1.3 seconds, 10ms a step. The same folder before these three
changes cost 21 seconds and came back a smear.

The guarantee is in world units, and the canvas is what shrinks them. Two
hundred notes fitted into a panel two hundred pixels wide are drawn as
overlapping discs however far apart they settled; that is a legibility limit
and what the node cap answers, not a correctness one. There is no quadtree: at
a neighbourhood's size the pairwise loop is cheaper than the tree that would
avoid it.

`simulate` runs exactly the step count it is given. There is no energy
threshold and no early stop, so a graph that never quiets down still costs one
settle rather than a frame loop with no end. `step` is exported so the caller
owns the animation frame, and a spent state steps to itself.

### 4. Canvas, not SVG and not WebGL

SVG puts a DOM node under every note and every link, and a settle mutates all
of them on every frame; the whole-folder view would be thousands of elements
being restyled sixty times a second inside a webview that also has an editor in
it. WebGL is the other end: a context, a shader pair and a text-rendering
problem, for a drawing whose largest form is capped in the low thousands of
nodes.

Canvas 2D is the fit. One element, one context, a draw call per frame, text
that comes from the platform's own shaper, and a size the app can hand it.

### 5. No third-party graph library

Nothing here is a rendering dependency. A graph library brings its own scene
graph, its own colours and its own idea of a theme; the drawing has to take
every colour from the app's tokens and follow a theme change, and the layout
has to be seeded and reproducible. Both are a few hundred lines against a
library that would have to be bent to do either.

### 6. Colour comes from tokens, read once per theme change

The canvas cannot inherit a custom property, so the component reads the tokens
it paints with off the canvas element through `getComputedStyle` and keeps
them: `--writ-bg-canvas` for the ground, `--writ-fg-muted` for the links,
`--writ-fg` for the open note's name, `--writ-accent` for the open note itself,
`--writ-bg-raised` and `--writ-border` for the notes around it. The read
happens on mount and again when the theme's resolved tokens change, never per
frame. No colour is written in the file, which is what the architecture test
over `src/` already enforces for CSS and now covers here too.

The whole-folder view spends one more token the same way. A note's colour is
its top-level folder's, and a folder's colour is `--writ-accent` turned round
the hue circle in OKLCH by the folder's place in the sorted list, a golden
angle a step, keeping the accent's lightness and chroma so no folder shouts and
none disappears. A note in the root of the notes folder is drawn in
`--writ-fg-muted`. There is no second palette to keep in step with a theme:
change the accent and every folder follows. `src/lib/graph/color.ts` is pure
and tested: sRGB to OKLCH and back, the turn, and the map from folders to
colours. A search dims the notes it does not name to `--writ-icon-opacity` and drops
their folder colour for `--writ-fg-faint`, because on the Windows and GNOME
shells that token is 1 and opacity alone would dim nothing.

### 7. Reduced motion paints once

`prefers-reduced-motion: reduce` paints one frame, at the end of the settle.
Nothing is seen moving, which is what the setting asks for, and the picture is
the same picture: the settle is the same fixed step count either way, only the
schedule differs.

Up to four hundred notes, which is every neighbourhood and most folders, that
settle runs synchronously and no animation frame is asked for at all. A whole
folder is seconds of arithmetic, and a window that answers nothing for seconds
is worse than the movement the setting asked to be spared, so above that count
the same settle runs in twelve-millisecond pieces across frames. Both paint
once.

Without the setting the settle is paced by the frame: as many steps as fit
eight milliseconds, up to eight of them, then a paint. A neighbourhood takes
its eight steps a frame and opens out over half a second; two thousand notes
take one step a frame and settle over about two seconds.

### 8. The node cap is the whole-folder view's, and it is stated

The whole-folder view renders at most 2000 nodes, taking the largest connected
component, and says in one line how many notes that is out of how many: `2000
of 2500 notes, the largest linked group`. A drawing that silently omits notes
is worse than one that draws fewer and says so. The cap belongs to that view;
this record is where it is written down.

Under the cap every note is drawn, linked or not: a folder is its notes, and a
note nothing links to is a fact worth seeing. Over it, the group of notes that
reach each other is what survives, because a drawing of a folder's links is
what the view is for and the notes outside the largest group say least about
them. Two groups of the same size are settled by their first path, and a group
larger than the cap on its own is cut down by how many notes each note touches,
ties by path. The same folder always draws the same notes, whatever order the
index listed them in.

What the cap is about is legibility and cost, not correctness: the settle holds
its minimum separation whatever it is handed (§3), and it is the fitting to the
canvas that turns a large folder into discs too small and too close to read.
Fitting has a bound of its own at the other end: a drawing is opened out to at
most 1.6x, so a two-note neighbourhood is not blown up until two notes read as
a whole folder. Both views inherit that.

## Consequences

- A note's neighbourhood looks the same every time it is opened, on every
  machine, and a test can assert the coordinates rather than that a canvas
  exists.
- Layout cost is bounded by the step count, so a folder shape that will not
  settle costs a fixed amount and then stops. At the cap that bound is about
  1.3 seconds of arithmetic, spread over frames.
- A theme change repaints from new tokens with no reload and no second palette.
- The drawing is never the only way to reach a note: in the panel every note it
  draws is a row above it, under "Links" or under "Links to this note"; in the
  whole-folder view the notes are the file tree the view is drawn over, which
  lists every one of them and is keyboard reachable. The canvas carries a name
  and a count for a reader who is not shown it, and the open note's name is
  left off the drawing rather than painted illegibly small when the drawing is
  fitted down.
- The layout module has no dependency, so it runs under a bare `node` as
  readily as in the app, which is what makes the cross-process check possible.
