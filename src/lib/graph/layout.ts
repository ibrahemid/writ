/**
 * Where the notes around one note sit, worked out here rather than in Rust.
 *
 * The graph's rows come from the index (ADR-036); this file only turns them
 * into geometry, which is a per-frame loop driving a canvas and so belongs on
 * the side of the bridge the canvas is on (ADR-037).
 *
 * Nothing here reads the document, the clock or `Math.random`. The same rows
 * and the same seed give the same coordinates on every machine, which is what
 * makes a layout something a test can hold. Only `+ - * /` and `Math.sqrt`
 * take part in a position: those are exact in IEEE-754 everywhere, while
 * `Math.pow` and the trigonometric functions are left to the engine and drift
 * by a bit between platforms.
 */

/** One note to place. */
export interface LayoutNode {
  path: string;
}

/** A link between two notes, undirected as far as the layout is concerned. */
export interface LayoutEdge {
  from: string;
  to: string;
}

export interface Point {
  x: number;
  y: number;
}

export interface LayoutOptions {
  /** The area to place into, in CSS pixels. */
  width: number;
  height: number;
  /** How many steps a settle takes. The loop never runs longer than this. */
  steps: number;
  /** How hard two notes push each other apart. */
  repulsion: number;
  /** How hard a link pulls its two notes together. */
  spring: number;
  /** The length a link is happy at. */
  springLength: number;
  /** How hard the middle pulls, which is what keeps the drawing on screen. */
  centring: number;
  /** How much of a step's speed survives into the next one. */
  damping: number;
  /** No two notes end up closer than this, centre to centre. */
  minSeparation: number;
  /** How far a note stays from the edge. */
  padding: number;
}

export const DEFAULT_LAYOUT_OPTIONS: LayoutOptions = {
  width: 216,
  height: 160,
  steps: 240,
  repulsion: 1400,
  spring: 0.05,
  springLength: 52,
  centring: 0.012,
  damping: 0.82,
  minSeparation: 22,
  padding: 14,
};

/**
 * How many notes a settle holds `minSeparation` for in the default area.
 *
 * The bound is measured rather than derived: how many notes fit depends on the
 * shape as much as on the area, and a folder's densest shape is the one where
 * everything links to everything, which packs worst. Held for a star, a ring
 * and a clique of this size across a seed sweep. Above it the notes still
 * settle, terminate and stay inside the area, but two of them may sit closer
 * than the minimum, and it is the drawing's node cap that answers that
 * (ADR-037).
 */
export const SEPARATION_HOLDS_TO = 12;

/**
 * A settle in progress: the paths in the order they were given, their
 * positions and speeds, and how far through the step count it is.
 *
 * Every array is indexed by the same position, and the path order is the order
 * `positions` walks, so the serialised result of one settle is the serialised
 * result of the next.
 */
export interface LayoutState {
  readonly options: LayoutOptions;
  readonly paths: readonly string[];
  readonly x: readonly number[];
  readonly y: readonly number[];
  readonly vx: readonly number[];
  readonly vy: readonly number[];
  /** Index pairs, so the force loop never looks a path up by name. */
  readonly links: readonly [number, number][];
  readonly stepsTaken: number;
  /** Whether the step count is spent. A settled state steps to itself. */
  readonly done: boolean;
}

/** How many times a step tries to satisfy the minimum separation. */
const SEPARATION_PASSES = 6;

/** How far a settled drawing may be opened out to fill its area. */
const MAX_FILL = 2.5;

/** Below this a pair counts as sitting on the same point. */
const COINCIDENT = 1e-9;

/**
 * A 32-bit linear congruential generator, the Numerical Recipes constants.
 *
 * `Math.imul` keeps the multiply exact at 32 bits, so the sequence is the same
 * one on every engine rather than the one a 53-bit float happens to round to.
 */
function nextSeed(seed: number): number {
  return (Math.imul(seed, 1664525) + 1013904223) | 0;
}

function unitOf(seed: number): number {
  return (seed >>> 0) / 4294967296;
}

/**
 * The seed a settle starts from. Zero is the one value the generator cannot
 * leave, so it is moved off rather than left to produce a stack of notes at
 * one point.
 */
function normaliseSeed(seed: number): number {
  const whole = Math.trunc(seed) | 0;
  return whole === 0 ? 0x2f6e2b1 : whole;
}

/**
 * A settle ready to run, with every note dropped into the middle of the area.
 *
 * The starting square is deliberately smaller than the canvas: the forces open
 * the drawing out from the middle, which reads as the note's neighbours
 * arriving rather than as a field collapsing inwards.
 */
export function beginLayout(
  nodes: readonly LayoutNode[],
  edges: readonly LayoutEdge[],
  options: LayoutOptions,
  seed: number,
): LayoutState {
  const paths = nodes.map((node) => node.path);
  const index = new Map<string, number>();
  paths.forEach((path, at) => index.set(path, at));

  const midX = options.width / 2;
  const midY = options.height / 2;
  const spreadX = options.width / 3;
  const spreadY = options.height / 3;

  const x: number[] = [];
  const y: number[] = [];
  let state = normaliseSeed(seed);
  for (let i = 0; i < paths.length; i += 1) {
    state = nextSeed(state);
    x.push(midX + (unitOf(state) - 0.5) * spreadX);
    state = nextSeed(state);
    y.push(midY + (unitOf(state) - 0.5) * spreadY);
  }

  const links: [number, number][] = [];
  for (const edge of edges) {
    const from = index.get(edge.from);
    const to = index.get(edge.to);
    if (from === undefined || to === undefined || from === to) continue;
    links.push([from, to]);
  }

  return {
    options,
    paths,
    x,
    y,
    vx: paths.map(() => 0),
    vy: paths.map(() => 0),
    links,
    stepsTaken: 0,
    done: paths.length === 0,
  };
}

/**
 * Pushes every pair at least `minSeparation` apart, in place.
 *
 * A pair sitting on exactly the same point has no direction to move along, so
 * one is derived from the two indices. It is not random: two runs of the same
 * settle separate the same pair the same way.
 */
function separate(x: number[], y: number[], options: LayoutOptions): void {
  const minX = options.padding;
  const maxX = options.width - options.padding;
  const minY = options.padding;
  const maxY = options.height - options.padding;
  const wanted = options.minSeparation;

  for (let pass = 0; pass < SEPARATION_PASSES; pass += 1) {
    let moved = false;
    for (let i = 0; i < x.length; i += 1) {
      for (let j = i + 1; j < x.length; j += 1) {
        let dx = x[j] - x[i];
        let dy = y[j] - y[i];
        let d2 = dx * dx + dy * dy;
        if (d2 < COINCIDENT) {
          dx = 1 + (j - i) / 64;
          dy = 1 - (j - i) / 64;
          d2 = dx * dx + dy * dy;
        }
        const d = Math.sqrt(d2);
        if (d >= wanted) continue;
        const share = (wanted - d) / 2 / d;
        const shiftX = dx * share;
        const shiftY = dy * share;
        x[i] = clamp(x[i] - shiftX, minX, maxX);
        y[i] = clamp(y[i] - shiftY, minY, maxY);
        x[j] = clamp(x[j] + shiftX, minX, maxX);
        y[j] = clamp(y[j] + shiftY, minY, maxY);
        moved = true;
      }
    }
    if (!moved) return;
  }
}

/**
 * Slides the whole drawing so what was placed sits in the middle of the area.
 *
 * Centring as a force only pulls towards the middle; where a graph comes to
 * rest is still decided by its shape, and a handful of notes reliably settles
 * off to one side with a third of the area empty. Moving the finished set is
 * the honest fix: every distance between two notes is untouched, and the shift
 * is held back so nothing crosses the padding it was already inside.
 */
function recentre(x: number[], y: number[], options: LayoutOptions): void {
  if (x.length === 0) return;
  let lowX = x[0];
  let highX = x[0];
  let lowY = y[0];
  let highY = y[0];
  for (let i = 1; i < x.length; i += 1) {
    if (x[i] < lowX) lowX = x[i];
    if (x[i] > highX) highX = x[i];
    if (y[i] < lowY) lowY = y[i];
    if (y[i] > highY) highY = y[i];
  }
  const minX = options.padding;
  const maxX = options.width - options.padding;
  const minY = options.padding;
  const maxY = options.height - options.padding;
  const shiftX = clamp((minX + maxX) / 2 - (lowX + highX) / 2, minX - lowX, maxX - highX);
  const shiftY = clamp((minY + maxY) / 2 - (lowY + highY) / 2, minY - lowY, maxY - highY);
  for (let i = 0; i < x.length; i += 1) {
    x[i] += shiftX;
    y[i] += shiftY;
  }
}

/**
 * Opens the drawing out until it fills the area it was given.
 *
 * The forces decide the shape; they do not decide the size, and what they
 * settle on depends on how many notes there are. Three notes huddle in a
 * quarter of the canvas and a dozen press against its edges. Scaling the
 * settled set about its own middle keeps every angle and every proportion and
 * lets a note's neighbourhood be legible at any count.
 *
 * Only ever larger, never smaller, so the minimum separation the pass before
 * this one just established cannot be undone here.
 */
function fill(x: number[], y: number[], options: LayoutOptions): void {
  if (x.length < 2) return;
  let lowX = x[0];
  let highX = x[0];
  let lowY = y[0];
  let highY = y[0];
  for (let i = 1; i < x.length; i += 1) {
    if (x[i] < lowX) lowX = x[i];
    if (x[i] > highX) highX = x[i];
    if (y[i] < lowY) lowY = y[i];
    if (y[i] > highY) highY = y[i];
  }
  const spanX = highX - lowX;
  const spanY = highY - lowY;
  const roomX = options.width - options.padding * 2;
  const roomY = options.height - options.padding * 2;
  const byX = spanX > COINCIDENT ? roomX / spanX : MAX_FILL;
  const byY = spanY > COINCIDENT ? roomY / spanY : MAX_FILL;
  let scale = byX < byY ? byX : byY;
  if (scale > MAX_FILL) scale = MAX_FILL;
  if (scale <= 1) return;
  const midX = (lowX + highX) / 2;
  const midY = (lowY + highY) / 2;
  for (let i = 0; i < x.length; i += 1) {
    x[i] = midX + (x[i] - midX) * scale;
    y[i] = midY + (y[i] - midY) * scale;
  }
}

function clamp(value: number, low: number, high: number): number {
  if (high < low) return low;
  if (value < low) return low;
  if (value > high) return high;
  return value;
}

/**
 * One step of the settle: repulsion between every pair, a spring along every
 * link, a pull towards the middle, then the separation constraint.
 *
 * A new state comes back rather than the old one edited, so a caller holding
 * the frame before this one still holds what it drew.
 */
export function step(state: LayoutState): LayoutState {
  if (state.done) return state;

  const { options } = state;
  const count = state.paths.length;
  const x = state.x.slice();
  const y = state.y.slice();
  const vx = state.vx.slice();
  const vy = state.vy.slice();
  const fx = new Array<number>(count).fill(0);
  const fy = new Array<number>(count).fill(0);

  for (let i = 0; i < count; i += 1) {
    for (let j = i + 1; j < count; j += 1) {
      let dx = x[j] - x[i];
      let dy = y[j] - y[i];
      let d2 = dx * dx + dy * dy;
      if (d2 < COINCIDENT) {
        dx = 1 + (j - i) / 64;
        dy = 1 - (j - i) / 64;
        d2 = dx * dx + dy * dy;
      }
      const d = Math.sqrt(d2);
      const push = options.repulsion / d2 / d;
      fx[i] -= dx * push;
      fy[i] -= dy * push;
      fx[j] += dx * push;
      fy[j] += dy * push;
    }
  }

  for (const [from, to] of state.links) {
    let dx = x[to] - x[from];
    let dy = y[to] - y[from];
    let d2 = dx * dx + dy * dy;
    if (d2 < COINCIDENT) {
      dx = 1 + (to - from) / 64;
      dy = 1 - (to - from) / 64;
      d2 = dx * dx + dy * dy;
    }
    const d = Math.sqrt(d2);
    const pull = (options.spring * (d - options.springLength)) / d;
    fx[from] += dx * pull;
    fy[from] += dy * pull;
    fx[to] -= dx * pull;
    fy[to] -= dy * pull;
  }

  const midX = options.width / 2;
  const midY = options.height / 2;
  for (let i = 0; i < count; i += 1) {
    fx[i] += (midX - x[i]) * options.centring;
    fy[i] += (midY - y[i]) * options.centring;
  }

  const minX = options.padding;
  const maxX = options.width - options.padding;
  const minY = options.padding;
  const maxY = options.height - options.padding;
  for (let i = 0; i < count; i += 1) {
    vx[i] = (vx[i] + fx[i]) * options.damping;
    vy[i] = (vy[i] + fy[i]) * options.damping;
    x[i] = clamp(x[i] + vx[i], minX, maxX);
    y[i] = clamp(y[i] + vy[i], minY, maxY);
  }

  separate(x, y, options);
  fill(x, y, options);
  recentre(x, y, options);

  const stepsTaken = state.stepsTaken + 1;
  return {
    ...state,
    x,
    y,
    vx,
    vy,
    stepsTaken,
    done: stepsTaken >= options.steps,
  };
}

/** Where each note sits, in the order the notes were given. */
export function positions(state: LayoutState): Map<string, Point> {
  const placed = new Map<string, Point>();
  state.paths.forEach((path, at) => placed.set(path, { x: state.x[at], y: state.y[at] }));
  return placed;
}

/**
 * Runs a whole settle and hands back where every note ended up.
 *
 * The loop is the step count and nothing else: no energy check, no early stop,
 * so a graph that never quiets down still costs one settle rather than a frame
 * loop that never ends.
 */
export function simulate(
  nodes: readonly LayoutNode[],
  edges: readonly LayoutEdge[],
  options: LayoutOptions,
  seed: number,
): Map<string, Point> {
  let state = beginLayout(nodes, edges, options, seed);
  while (!state.done) state = step(state);
  return positions(state);
}

/** A placed note, as the canvas draws it. */
export interface PlacedNode extends Point {
  path: string;
  radius: number;
}

/**
 * The note under a point, or `null` for empty ground.
 *
 * Kept out of the component so that what a click means is testable without a
 * canvas, a pointer or a rectangle jsdom refuses to measure. Nearest wins, so
 * two overlapping discs cannot both answer.
 */
export function nodeAt(placed: readonly PlacedNode[], point: Point, slop = 4): string | null {
  let best: string | null = null;
  let bestDistance = 0;
  for (const node of placed) {
    const dx = point.x - node.x;
    const dy = point.y - node.y;
    const distance = Math.sqrt(dx * dx + dy * dy);
    if (distance > node.radius + slop) continue;
    if (best === null || distance < bestDistance) {
      best = node.path;
      bestDistance = distance;
    }
  }
  return best;
}
