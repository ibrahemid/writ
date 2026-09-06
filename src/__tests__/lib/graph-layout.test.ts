import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { describe, it, expect } from "vitest";
import {
  DEFAULT_LAYOUT_OPTIONS,
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
