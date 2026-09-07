import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { describe, it, expect } from "vitest";
import {
  DEFAULT_LAYOUT_OPTIONS,
  FOLDER_LAYOUT_OPTIONS,
  beginLayout,
  nodeAt,
  positions,
  simulate,
  step,
  type LayoutEdge,
  type LayoutNode,
  type PlacedNode,
} from "../../lib/graph/layout";

// A settle is a pure function of its rows and its seed. That is the property
// the drawing rests on: a note opened twice has to look the same both times,
// and a settle that runs on a machine this one cannot see has to agree.

const NODES: LayoutNode[] = [
  { path: "a.md" },
  { path: "b.md" },
  { path: "c.md" },
  { path: "d.md" },
  { path: "e.md" },
  { path: "f.md" },
  { path: "g.md" },
  { path: "h.md" },
  { path: "i.md" },
  { path: "j.md" },
];

const EDGES: LayoutEdge[] = [
  { from: "a.md", to: "b.md" },
  { from: "a.md", to: "c.md" },
  { from: "a.md", to: "d.md" },
  { from: "b.md", to: "c.md" },
  { from: "d.md", to: "e.md" },
  { from: "e.md", to: "f.md" },
  { from: "f.md", to: "g.md" },
  { from: "g.md", to: "h.md" },
  { from: "h.md", to: "i.md" },
];

const SEED = 20260906;

function serialise(placed: Map<string, { x: number; y: number }>): string {
  return JSON.stringify([...placed]);
}

describe("graph layout is deterministic", () => {
  it("the same rows and the same seed settle to the same coordinates", () => {
    const once = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED);
    const twice = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED);
    expect(serialise(twice)).toBe(serialise(once));
    expect([...once.keys()]).toEqual(NODES.map((node) => node.path));
  });

  it("another seed settles somewhere else", () => {
    const here = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED);
    const there = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED + 1);
    expect(serialise(there)).not.toBe(serialise(here));
  });

  it(
    "a second process settles to the same coordinates",
    () => {
      const layout = pathToFileURL(resolve(process.cwd(), "src/lib/graph/layout.ts")).href;
      const script = [
        `const m = await import(${JSON.stringify(layout)});`,
        `const placed = m.simulate(${JSON.stringify(NODES)}, ${JSON.stringify(EDGES)}, m.DEFAULT_LAYOUT_OPTIONS, ${SEED});`,
        "process.stdout.write(JSON.stringify([...placed]));",
      ].join("\n");
      const elsewhere = execFileSync(process.execPath, ["--input-type=module", "-e", script], {
        encoding: "utf8",
      });
      expect(elsewhere).toBe(serialise(simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED)));
    },
    // Starting a second node process costs what the machine has left to give.
    120_000,
  );
});

describe("a settled graph", () => {
  it("holds every pair at the minimum separation", () => {
    const placed = [...simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED).values()];
    expect(closestPair(placed)).toBeGreaterThanOrEqual(
      DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6,
    );
  });

  it("settles around the middle of the world rather than drifting off", () => {
    const placed = [...simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED).values()];
    const xs = placed.map((point) => point.x);
    const ys = placed.map((point) => point.y);
    const midX = (Math.min(...xs) + Math.max(...xs)) / 2;
    const midY = (Math.min(...ys) + Math.max(...ys)) / 2;
    const span = Math.max(Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys));
    expect(Math.abs(midX)).toBeLessThan(span);
    expect(Math.abs(midY)).toBeLessThan(span);
  });
});

/** A hub note: everything links to the first note and to nothing else. */
function star(count: number): { nodes: LayoutNode[]; edges: LayoutEdge[] } {
  const nodes = Array.from({ length: count }, (_, i) => ({ path: `n${i}.md` }));
  const edges = Array.from({ length: count - 1 }, (_, i) => ({
    from: "n0.md",
    to: `n${i + 1}.md`,
  }));
  return { nodes, edges };
}

/** A loop: each note links to the next and the last back to the first. */
function ring(count: number): { nodes: LayoutNode[]; edges: LayoutEdge[] } {
  const nodes = Array.from({ length: count }, (_, i) => ({ path: `n${i}.md` }));
  const edges = nodes.map((node, i) => ({ from: node.path, to: nodes[(i + 1) % count].path }));
  return { nodes, edges };
}

/** The densest shape a folder can make: everything links to everything. */
function clique(count: number): { nodes: LayoutNode[]; edges: LayoutEdge[] } {
  const nodes = Array.from({ length: count }, (_, i) => ({ path: `n${i}.md` }));
  const edges: LayoutEdge[] = [];
  for (let i = 0; i < count; i += 1) {
    for (let j = i + 1; j < count; j += 1) edges.push({ from: `n${i}.md`, to: `n${j}.md` });
  }
  return { nodes, edges };
}

/**
 * The two notes sitting closest together.
 *
 * A settle that blows up leaves NaN coordinates, and every distance between
 * them is NaN too, which no comparison is ever true for. That would leave the
 * closest pair reading as infinity — a settle that produced nothing at all
 * passing the test for a settle that held every note apart. A run that lost a
 * coordinate reports NaN, which fails.
 */
function closestPair(placed: { x: number; y: number }[]): number {
  for (const point of placed) {
    if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) return Number.NaN;
  }
  let closest = Number.POSITIVE_INFINITY;
  for (let i = 0; i < placed.length; i += 1) {
    for (let j = i + 1; j < placed.length; j += 1) {
      const dx = placed[j].x - placed[i].x;
      const dy = placed[j].y - placed[i].y;
      closest = Math.min(closest, Math.sqrt(dx * dx + dy * dy));
    }
  }
  return closest;
}

/** Runs a whole settle and says how many steps it took to get there. */
function settle(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  seed: number,
  options = DEFAULT_LAYOUT_OPTIONS,
): { placed: { x: number; y: number }[]; steps: number } {
  let state = beginLayout(nodes, edges, options, seed);
  let steps = 0;
  while (!state.done && steps <= options.steps) {
    state = step(state);
    steps += 1;
  }
  return { placed: [...positions(state).values()], steps };
}

describe("the minimum separation", () => {
  // The settle works in world units and nothing bounds it, so how many notes
  // are being placed is not a thing the minimum can run out of room for. A hub
  // note and a folder where everything links to everything are the two shapes
  // that pull hardest against it, and it holds for both at every size.
  const shapes = { star, clique };
  const sizes = [
    { count: 8, seeds: 20 },
    { count: 12, seeds: 20 },
    { count: 64, seeds: 2 },
    { count: 200, seeds: 2 },
  ];

  // A settle of a few hundred notes is a second of arithmetic on a quiet
  // machine and several on one running the rest of the suite beside it, so
  // the sizes that cost anything say how long they may take rather than
  // inheriting the default and failing on a busy machine.
  const SETTLE_TIMEOUT_MS = 120_000;

  for (const [name, shape] of Object.entries(shapes)) {
    for (const { count, seeds } of sizes) {
      it(
        `holds every pair apart in a ${name} of ${count}`,
        () => {
          const { nodes, edges } = shape(count);
          for (let seed = 1; seed <= seeds; seed += 1) {
            const run = settle(nodes, edges, seed);
            expect(closestPair(run.placed)).toBeGreaterThanOrEqual(
              DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6,
            );
            expect(run.steps).toBe(DEFAULT_LAYOUT_OPTIONS.steps);
          }
        },
        SETTLE_TIMEOUT_MS,
      );
    }
  }

  // Past a few hundred notes the forces alone stop being enough: the springs
  // and the repulsion balance closer than the minimum, and what holds the
  // notes off each other is the room the drawing is opened out to and the pass
  // that pushes the rest apart. This is the size that says whether that is
  // doing its job. One shape per test: each is seconds of arithmetic, and two
  // of them in one test is one test that takes twice as long to fail.
  for (const [name, shape] of Object.entries(shapes)) {
    it(
      `holds a folder far larger than one is drawn at apart, in a ${name}`,
      () => {
        const { nodes, edges } = shape(400);
        const run = settle(nodes, edges, 1);
        expect(closestPair(run.placed)).toBeGreaterThanOrEqual(
          DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6,
        );
        expect(run.steps).toBe(DEFAULT_LAYOUT_OPTIONS.steps);
      },
      SETTLE_TIMEOUT_MS,
    );
  }

  it("holds a loop apart too", () => {
    const { nodes, edges } = ring(12);
    for (let seed = 1; seed <= 20; seed += 1) {
      const placed = [...simulate(nodes, edges, DEFAULT_LAYOUT_OPTIONS, seed).values()];
      expect(closestPair(placed)).toBeGreaterThanOrEqual(
        DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6,
      );
    }
  });

  // Everything above settles with the numbers the notes around one note are
  // settled with. A whole folder is settled with its own, which ask for a
  // closer minimum in half the steps: whether the minimum still holds is a
  // property of those numbers rather than of the ones a dozen notes get.
  for (const [name, shape] of Object.entries(shapes)) {
    it(
      `holds a folder apart at the numbers a folder is settled with, in a ${name}`,
      () => {
        const { nodes, edges } = shape(400);
        const run = settle(nodes, edges, 1, FOLDER_LAYOUT_OPTIONS);
        expect(closestPair(run.placed)).toBeGreaterThanOrEqual(
          FOLDER_LAYOUT_OPTIONS.minSeparation - 1e-6,
        );
        expect(run.steps).toBe(FOLDER_LAYOUT_OPTIONS.steps);
      },
      SETTLE_TIMEOUT_MS,
    );
  }
});

// A folder is one drawing that notes are written into and deleted from, and
// the drawing is on screen while that happens: a note arriving may not be a
// reason for the folder to rearrange itself around it.
describe("a settle handed where the notes already were", () => {
  const WITH_ONE_MORE: LayoutNode[] = [...NODES, { path: "k.md" }];

  it("starts every note that is still there exactly where it was", () => {
    const before = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED);
    const started = positions(
      beginLayout(WITH_ONE_MORE, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED, before),
    );

    for (const node of NODES) expect(started.get(node.path)).toEqual(before.get(node.path));
    const fresh = started.get("k.md");
    expect(fresh).toBeDefined();
    expect(Number.isFinite(fresh!.x) && Number.isFinite(fresh!.y)).toBe(true);
  });

  it("settles from there into a drawing that still holds every pair apart", () => {
    const before = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED);
    let state = beginLayout(WITH_ONE_MORE, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED, before);
    while (!state.done) state = step(state);

    const placed = [...positions(state).values()];
    expect(placed.length).toBe(WITH_ONE_MORE.length);
    expect(closestPair(placed)).toBeGreaterThanOrEqual(DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6);
  });

  it("leaves behind a note the folder no longer holds", () => {
    const before = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED);
    const fewer = NODES.slice(0, NODES.length - 1);
    const started = positions(beginLayout(fewer, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED, before));
    expect([...started.keys()]).toEqual(fewer.map((node) => node.path));
  });

  it("settles the same set the same way it did before, handed its own answer", () => {
    const before = simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED);
    const started = positions(beginLayout(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED, before));
    expect(serialise(started)).toBe(serialise(before));
  });
});

describe("the settle terminates", () => {
  it("ends after the step count it was given and steps to itself after that", () => {
    const options = { ...DEFAULT_LAYOUT_OPTIONS, steps: 40 };
    let state = beginLayout(NODES, EDGES, options, SEED);
    let taken = 0;
    while (!state.done) {
      state = step(state);
      taken += 1;
      expect(taken).toBeLessThanOrEqual(options.steps);
    }
    expect(taken).toBe(options.steps);
    expect(state.stepsTaken).toBe(options.steps);
    const after = step(state);
    expect(after).toBe(state);
    expect(serialise(positions(after))).toBe(serialise(positions(state)));
  });
});

describe("the edge cases of a settle", () => {
  it("an empty graph places nothing", () => {
    const placed = simulate([], [], DEFAULT_LAYOUT_OPTIONS, SEED);
    expect(placed.size).toBe(0);
    expect(beginLayout([], [], DEFAULT_LAYOUT_OPTIONS, SEED).done).toBe(true);
  });

  it("a note with no links is placed rather than dropped", () => {
    const nodes: LayoutNode[] = [{ path: "a.md" }, { path: "b.md" }, { path: "alone.md" }];
    const placed = simulate(nodes, [{ from: "a.md", to: "b.md" }], DEFAULT_LAYOUT_OPTIONS, SEED);
    expect(placed.size).toBe(3);
    const alone = placed.get("alone.md");
    expect(alone).toBeDefined();
    expect(Number.isFinite(alone?.x)).toBe(true);
    expect(Number.isFinite(alone?.y)).toBe(true);
  });

  it("a link naming a note that is not here is not drawn", () => {
    const nodes: LayoutNode[] = [{ path: "a.md" }];
    const state = beginLayout(nodes, [{ from: "a.md", to: "gone.md" }], DEFAULT_LAYOUT_OPTIONS, SEED);
    expect(state.links).toEqual([]);
  });

  it("two notes with no link between them are still held apart", () => {
    const placed = [
      ...simulate([{ path: "a.md" }, { path: "b.md" }], [], DEFAULT_LAYOUT_OPTIONS, SEED).values(),
    ];
    const dx = placed[1].x - placed[0].x;
    const dy = placed[1].y - placed[0].y;
    expect(Math.sqrt(dx * dx + dy * dy)).toBeGreaterThanOrEqual(
      DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6,
    );
  });
});

describe("what a pointer is over", () => {
  const placed: PlacedNode[] = [
    { path: "a.md", x: 20, y: 20, radius: 6 },
    { path: "b.md", x: 24, y: 22, radius: 6 },
    { path: "far.md", x: 120, y: 90, radius: 4 },
  ];

  it("names the note under the point", () => {
    expect(nodeAt(placed, { x: 120, y: 90 })).toBe("far.md");
  });

  it("names the nearer of two that overlap", () => {
    expect(nodeAt(placed, { x: 25, y: 22 })).toBe("b.md");
  });

  it("names nothing over empty ground", () => {
    expect(nodeAt(placed, { x: 200, y: 10 })).toBeNull();
  });
});
