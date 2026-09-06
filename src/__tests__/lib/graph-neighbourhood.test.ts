import { describe, it, expect } from "vitest";
import { neighbourhoodOf } from "../../lib/graph/neighbourhood";

// What the drawing is cut from: the folder's whole graph, narrowed to the open
// note and one step out, with the links among those neighbours kept.

const NODES = [
  { path: "Alpha.md", name: "Alpha" },
  { path: "Beta.md", name: "Beta" },
  { path: "Gamma.md", name: "Gamma" },
  { path: "Delta.md", name: "Delta" },
  { path: "Far.md", name: "Far" },
];

const EDGES = [
  { from_path: "Alpha.md", to_path: "Beta.md" },
  { from_path: "Gamma.md", to_path: "Alpha.md" },
  { from_path: "Beta.md", to_path: "Gamma.md" },
  { from_path: "Delta.md", to_path: "Far.md" },
];

describe("the neighbourhood of a note", () => {
  it("holds the note and everything one link away, either direction", () => {
    const near = neighbourhoodOf(NODES, EDGES, "Alpha.md");
    expect(near.nodes.map((node) => node.path)).toEqual(["Alpha.md", "Beta.md", "Gamma.md"]);
    expect(near.nodes[0].name).toBe("Alpha");
  });

  it("keeps a link between two neighbours, which is what makes it not a star", () => {
    const near = neighbourhoodOf(NODES, EDGES, "Alpha.md");
    expect(near.edges).toContainEqual({ from: "Beta.md", to: "Gamma.md" });
    expect(near.edges).toHaveLength(3);
    expect(near.nodes.map((node) => node.degree)).toEqual([2, 2, 2]);
  });

  it("leaves out a note two steps away", () => {
    const near = neighbourhoodOf(NODES, EDGES, "Alpha.md");
    expect(near.nodes.map((node) => node.path)).not.toContain("Far.md");
  });

  it("counts a pair linked both ways once", () => {
    const near = neighbourhoodOf(NODES, [...EDGES, { from_path: "Beta.md", to_path: "Alpha.md" }], "Alpha.md");
    expect(near.edges.filter((edge) => edge.from === "Alpha.md" && edge.to === "Beta.md")).toHaveLength(1);
  });

  it("reads empty for a note nothing links to and that links to nothing", () => {
    const near = neighbourhoodOf(NODES, EDGES, "Alpha.md");
    expect(near.nodes.length).toBeGreaterThan(0);
    const alone = neighbourhoodOf(NODES, EDGES, "Beta.md");
    expect(alone.nodes.length).toBeGreaterThan(0);
    expect(neighbourhoodOf(NODES, [{ from_path: "Delta.md", to_path: "Far.md" }], "Alpha.md")).toEqual({
      nodes: [],
      edges: [],
    });
  });

  it("reads empty for a note the index does not hold", () => {
    expect(neighbourhoodOf(NODES, EDGES, "Missing.md")).toEqual({ nodes: [], edges: [] });
  });

  it("drops a link a note writes to itself", () => {
    const near = neighbourhoodOf(NODES, [{ from_path: "Alpha.md", to_path: "Alpha.md" }], "Alpha.md");
    expect(near).toEqual({ nodes: [], edges: [] });
  });

  it("orders the neighbours the same way every time", () => {
    const forwards = neighbourhoodOf(NODES, EDGES, "Alpha.md");
    const backwards = neighbourhoodOf([...NODES].reverse(), [...EDGES].reverse(), "Alpha.md");
    expect(backwards.nodes.map((node) => node.path)).toEqual(
      forwards.nodes.map((node) => node.path),
    );
  });
});
