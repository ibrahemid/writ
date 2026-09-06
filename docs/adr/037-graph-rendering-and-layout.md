# ADR-037: Where a graph is worked out and what draws it

## Status

Accepted.

## Context

Two surfaces draw the folder's link graph: the panel beside the note, which
shows the open note and everything one link away from it, and the whole-folder
view that follows it. Both take their rows from the read model in ADR-036, and
both have to turn those rows into coordinates and then into pixels.

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

### 3. The forces are repulsion, springs and centring, and the step count is fixed

Every pair pushes apart, every link pulls together, the middle pulls everything
in, and a positional pass afterwards holds every pair at a minimum separation
so two notes cannot end up as one dot. There is no quadtree: at a
neighbourhood's size the pairwise loop is cheaper than the tree that would
avoid it, and the whole-folder view caps its node count instead.

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

### 7. Reduced motion settles at once

`prefers-reduced-motion: reduce` runs the whole settle synchronously and paints
one frame. No animation frame is asked for, so there is nothing to interrupt
and nothing to see moving. The picture is the same picture: the settle is the
same fixed step count either way, only the schedule differs.

### 8. The node cap is the whole-folder view's, and it is stated

The neighbourhood drawing is bounded by the folder itself: one note's
neighbours are a handful. The whole-folder view is not, so it renders at most
2000 nodes, taking the largest connected component, and says in one line how
many notes that is out of how many. A drawing that silently omits notes is
worse than one that draws fewer and says so. The cap belongs to that view; this
record is where it is written down.

The other bound on size is the settle's own: after each step the drawing is
scaled about its middle to fill the area it was given, by at most 2.5x and
never by less than 1x, so a two-note neighbourhood opens out without a pair of
notes being pulled back onto each other. Both views inherit that.

## Consequences

- A note's neighbourhood looks the same every time it is opened, on every
  machine, and a test can assert the coordinates rather than that a canvas
  exists.
- Layout cost is bounded by the step count, so a folder shape that will not
  settle costs a fixed amount and then stops.
- A theme change repaints from new tokens with no reload and no second palette.
- The drawing is never the only way to reach a note. The same neighbours are
  listed as text in the panel above it, and the canvas carries a name and a
  count for a reader who is not shown it.
- The layout module has no dependency, so it runs under a bare `node` as
  readily as in the app, which is what makes the cross-process check possible.
