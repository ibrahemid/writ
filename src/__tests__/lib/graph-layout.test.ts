import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { describe, it, expect } from "vitest";
import {
  DEFAULT_LAYOUT_OPTIONS,
  SEPARATION_HOLDS_TO,
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
    30_000,
  );
});

describe("a settled graph", () => {
  it("holds every pair at the minimum separation", () => {
    const placed = [...simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED).values()];
    let closest = Number.POSITIVE_INFINITY;
    for (let i = 0; i < placed.length; i += 1) {
      for (let j = i + 1; j < placed.length; j += 1) {
        const dx = placed[j].x - placed[i].x;
        const dy = placed[j].y - placed[i].y;
        closest = Math.min(closest, Math.sqrt(dx * dx + dy * dy));
      }
    }
    expect(closest).toBeGreaterThanOrEqual(DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6);
  });

  it("sits in the middle of the area rather than off to one side", () => {
    const { width, height } = DEFAULT_LAYOUT_OPTIONS;
    const placed = [...simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED).values()];
    const xs = placed.map((point) => point.x);
    const ys = placed.map((point) => point.y);
    const midX = (Math.min(...xs) + Math.max(...xs)) / 2;
    const midY = (Math.min(...ys) + Math.max(...ys)) / 2;
    expect(Math.abs(midX - width / 2)).toBeLessThan(1);
    expect(Math.abs(midY - height / 2)).toBeLessThan(1);
  });

  it("opens out to fill the area rather than huddling in the middle", () => {
    const { width, height, padding } = DEFAULT_LAYOUT_OPTIONS;
    const placed = [...simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED).values()];
    const xs = placed.map((point) => point.x);
    const ys = placed.map((point) => point.y);
    const spanX = (Math.max(...xs) - Math.min(...xs)) / (width - padding * 2);
    const spanY = (Math.max(...ys) - Math.min(...ys)) / (height - padding * 2);
    expect(Math.max(spanX, spanY)).toBeGreaterThan(0.9);
  });

  it("holds both when a shape leans on one side of the area", () => {
    // A chain is the shape that opens out furthest before it meets an edge, so
    // it is where scaling to fill the area could push a note past the padding
    // or back onto its neighbour. Neither may happen, on any seed.
    const nodes: LayoutNode[] = [
      { path: "a.md" },
      { path: "b.md" },
      { path: "c.md" },
      { path: "d.md" },
    ];
    const edges: LayoutEdge[] = [
      { from: "a.md", to: "b.md" },
      { from: "b.md", to: "c.md" },
      { from: "c.md", to: "d.md" },
    ];
    const { width, height, padding, minSeparation } = DEFAULT_LAYOUT_OPTIONS;
    for (let seed = 1; seed <= 60; seed += 1) {
      const placed = [...simulate(nodes, edges, DEFAULT_LAYOUT_OPTIONS, seed).values()];
      for (let i = 0; i < placed.length; i += 1) {
        expect(placed[i].x).toBeGreaterThanOrEqual(padding - 1e-6);
        expect(placed[i].x).toBeLessThanOrEqual(width - padding + 1e-6);
        expect(placed[i].y).toBeGreaterThanOrEqual(padding - 1e-6);
        expect(placed[i].y).toBeLessThanOrEqual(height - padding + 1e-6);
        for (let j = i + 1; j < placed.length; j += 1) {
          const dx = placed[j].x - placed[i].x;
          const dy = placed[j].y - placed[i].y;
          expect(Math.sqrt(dx * dx + dy * dy)).toBeGreaterThanOrEqual(minSeparation - 1e-6);
        }
      }
    }
  });

  it("stays inside the area it was given", () => {
    const { width, height, padding } = DEFAULT_LAYOUT_OPTIONS;
    for (const point of simulate(NODES, EDGES, DEFAULT_LAYOUT_OPTIONS, SEED).values()) {
      expect(point.x).toBeGreaterThanOrEqual(padding - 1e-6);
      expect(point.x).toBeLessThanOrEqual(width - padding + 1e-6);
      expect(point.y).toBeGreaterThanOrEqual(padding - 1e-6);
      expect(point.y).toBeLessThanOrEqual(height - padding + 1e-6);
    }
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

function closestPair(placed: { x: number; y: number }[]): number {
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

describe("the minimum separation", () => {
  // These are the shapes the springs pull tight enough that pushing apart is
  // the only thing holding the notes off each other: with the separation pass
  // gone they settle on top of one another, which is what the pass exists to
  // stop.
  const shapes = { star, ring, clique };

  for (const [name, shape] of Object.entries(shapes)) {
    it(`holds every pair apart in a ${name} of ${SEPARATION_HOLDS_TO}`, () => {
      const { nodes, edges } = shape(SEPARATION_HOLDS_TO);
      for (let seed = 1; seed <= 40; seed += 1) {
        const placed = [...simulate(nodes, edges, DEFAULT_LAYOUT_OPTIONS, seed).values()];
        expect(closestPair(placed)).toBeGreaterThanOrEqual(
          DEFAULT_LAYOUT_OPTIONS.minSeparation - 1e-6,
        );
      }
    });
  }
});

describe("opening the settle out to the area", () => {
  it("takes a linked pair further apart than the link would leave them", () => {
    // Two linked notes rest at the length of the link, which is a third of
    // the area. Nothing but the fill pass takes them further.
    const nodes: LayoutNode[] = [{ path: "a.md" }, { path: "b.md" }];
    const edges: LayoutEdge[] = [{ from: "a.md", to: "b.md" }];
    for (let seed = 1; seed <= 20; seed += 1) {
      const placed = [...simulate(nodes, edges, DEFAULT_LAYOUT_OPTIONS, seed).values()];
      expect(closestPair(placed)).toBeGreaterThan(DEFAULT_LAYOUT_OPTIONS.springLength * 2);
    }
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
