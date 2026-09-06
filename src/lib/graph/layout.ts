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
}

export const DEFAULT_LAYOUT_OPTIONS: LayoutOptions = {
  steps: 240,
  repulsion: 1400,
  spring: 0.05,
  springLength: 52,
  centring: 0.012,
  damping: 0.82,
  minSeparation: 22,
};

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

/**
 * How many times a step tries to satisfy the minimum separation.
 *
 * A pass walks every pair and pushes the ones sitting too close apart, which
 * can leave a pair it already passed too close again, so it runs until a whole
 * pass moves nothing. A settled drawing costs one pass; the cap is what a step
 * that cannot satisfy every pair at once stops at rather than running forever.
 */
const SEPARATION_PASSES = 64;

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
 * A settle ready to run, with every note dropped near the origin.
 *
 * Coordinates are world units and nothing bounds them: the canvas fits what
 * comes back into whatever room it has (`fitToView`), so the settle answers
 * how the notes sit relative to each other and only that. The minimum
 * separation is then a property of the settle and holds however many notes
 * there are, rather than one an area can run out of room for.
 *
 * Every note starts inside one square about a link across; the forces open the
 * drawing out from there, which reads as the note's neighbours arriving rather
 * than as a field collapsing inwards.
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

  const spread = options.springLength;

  const x: number[] = [];
  const y: number[] = [];
  let state = normaliseSeed(seed);
  for (let i = 0; i < paths.length; i += 1) {
    state = nextSeed(state);
    x.push((unitOf(state) - 0.5) * spread);
    state = nextSeed(state);
    y.push((unitOf(state) - 0.5) * spread);
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
        x[i] -= shiftX;
        y[i] -= shiftY;
        x[j] += shiftX;
        y[j] += shiftY;
        moved = true;
      }
    }
    if (!moved) return;
  }
}

/**
 * One step of the settle: repulsion between every pair, a spring along every
 * link, a pull towards the origin, then the separation constraint.
 *
 * A note's springs are shared out over its links rather than summed. A note
 * linked to two others is pulled by two springs; one linked to two hundred
 * would be pulled two hundred times as hard, and no separation pass can undo
 * that every step. Sharing them out lets a folder where everything links to
 * everything settle into a packing rather than a pile, which is what makes
 * the minimum separation hold at any size.
 *
 * Separation runs last, so the positions a step ends on are the ones it just
 * pushed apart. That is also what keeps the arithmetic finite: repulsion goes
 * as one over the distance cubed, and a step that ends with no pair closer
 * than the minimum is a step the next one's forces are bounded by.
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

  const sx = new Array<number>(count).fill(0);
  const sy = new Array<number>(count).fill(0);
  const degree = new Array<number>(count).fill(0);
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
    sx[from] += dx * pull;
    sy[from] += dy * pull;
    sx[to] -= dx * pull;
    sy[to] -= dy * pull;
    degree[from] += 1;
    degree[to] += 1;
  }

  for (let i = 0; i < count; i += 1) {
    const links = degree[i] > 1 ? degree[i] : 1;
    fx[i] += sx[i] / links;
    fy[i] += sy[i] / links;
  }

  for (let i = 0; i < count; i += 1) {
    fx[i] -= x[i] * options.centring;
    fy[i] -= y[i] * options.centring;
  }

  for (let i = 0; i < count; i += 1) {
    vx[i] = (vx[i] + fx[i]) * options.damping;
    vy[i] = (vy[i] + fy[i]) * options.damping;
    x[i] += vx[i];
    y[i] += vy[i];
  }

  separate(x, y, options);

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
