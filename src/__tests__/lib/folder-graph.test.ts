import { describe, it, expect } from "vitest";
import {
  NODE_CAP,
  countMatches,
  fileGraphOf,
  folderGraphOf,
  folderLegend,
  graphMatches,
  matchesQuery,
  stepMatch,
  type FolderEdgeRow,
  type FolderNodeRow,
} from "../../lib/graph/folder-graph";

// What a folder's drawing is made of. Under the cap it is every note the index
// holds; over it, the largest group of notes that link to each other, cut down
// by how many links each note carries if that group is over the cap on its own.

function notes(count: number, folder = "", from = 0): FolderNodeRow[] {
  return Array.from({ length: count }, (_, i) => ({
    path: `${folder}${folder ? "/" : ""}n${String(from + i).padStart(5, "0")}.md`,
    name: `Note ${from + i}`,
    folder,
  }));
}

function chain(rows: FolderNodeRow[]): FolderEdgeRow[] {
  return rows.slice(1).map((row, i) => ({ from_path: rows[i].path, to_path: row.path }));
}

describe("a folder under the cap", () => {
  const ROWS: FolderNodeRow[] = [
    { path: "Archive/old.md", name: "Old", folder: "Archive" },
    { path: "Projects/writ.md", name: "Writ", folder: "Projects" },
    { path: "alone.md", name: "Alone", folder: "" },
  ];
  const EDGES: FolderEdgeRow[] = [
    { from_path: "Projects/writ.md", to_path: "Archive/old.md" },
    { from_path: "Archive/old.md", to_path: "Projects/writ.md" },
    { from_path: "alone.md", to_path: "alone.md" },
  ];

  it("draws every note, linked or not", () => {
    const graph = folderGraphOf(ROWS, EDGES);
    expect(graph.nodes.map((node) => node.path)).toEqual([
      "Archive/old.md",
      "Projects/writ.md",
      "alone.md",
    ]);
    expect(graph.total).toBe(3);
    expect(graph.capped).toBe(false);
  });

  it("draws one line for two notes that link both ways, and none to itself", () => {
    const graph = folderGraphOf(ROWS, EDGES);
    expect(graph.edges).toEqual([{ from: "Projects/writ.md", to: "Archive/old.md" }]);
    expect(graph.nodes.find((node) => node.path === "alone.md")?.degree).toBe(0);
    expect(graph.nodes.find((node) => node.path === "Archive/old.md")?.degree).toBe(1);
  });

  it("carries the folder a note is in, and no folder for one in the root", () => {
    const graph = folderGraphOf(ROWS, EDGES);
    expect(graph.nodes.map((node) => node.folder)).toEqual(["Archive", "Projects", ""]);
  });

  it("draws nothing for a folder with no notes", () => {
    const graph = folderGraphOf([], []);
    expect(graph.nodes).toEqual([]);
    expect(graph.total).toBe(0);
    expect(graph.capped).toBe(false);
  });

  it("draws no line to a note the index does not hold", () => {
    const graph = folderGraphOf(ROWS, [{ from_path: "Archive/old.md", to_path: "gone.md" }]);
    expect(graph.edges).toEqual([]);
  });
});

describe("a folder over the cap", () => {
  it("draws the largest group of notes that link to each other", { timeout: 60_000 }, () => {
    const big = notes(30, "Big");
    const small = notes(10, "Small");
    const loose = notes(NODE_CAP, "Loose");
    const graph = folderGraphOf([...loose, ...small, ...big], [...chain(big), ...chain(small)], 25);

    expect(graph.capped).toBe(true);
    expect(graph.total).toBe(NODE_CAP + 40);
    expect(graph.nodes.every((node) => node.folder === "Big")).toBe(true);
  });

  it("cuts a group larger than the cap down to the notes carrying the links", () => {
    // A hub of ten notes every other note links to, and a long chain past it.
    const hub = notes(10, "Hub");
    const rest = notes(40, "Rest");
    const edges: FolderEdgeRow[] = [];
    for (const spoke of rest) {
      for (const centre of hub) edges.push({ from_path: centre.path, to_path: spoke.path });
    }
    const graph = folderGraphOf([...hub, ...rest], edges, 12);

    expect(graph.nodes.length).toBe(12);
    expect(graph.nodes.filter((node) => node.folder === "Hub").length).toBe(10);
  });

  it("answers the same way whichever order the index listed the notes in", () => {
    const first = notes(20, "A");
    const second = notes(20, "B");
    const edges = [...chain(first), ...chain(second)];
    const one = folderGraphOf([...first, ...second], edges, 15);
    const other = folderGraphOf([...second].reverse().concat([...first].reverse()), edges, 15);
    expect(other.nodes.map((node) => node.path)).toEqual(one.nodes.map((node) => node.path));
  });

  // Sizing the timeout rather than inheriting the default: the fixture is
  // thousands of rows, and a machine running the rest of the suite beside this
  // one takes longer over them than a quiet one.
  it(
    "keeps two thousand of two and a half thousand notes by default",
    { timeout: 60_000 },
    () => {
      const linked = notes(2100, "Linked");
      const loose = notes(400, "Loose", 2100);
      const graph = folderGraphOf([...linked, ...loose], chain(linked));

      expect(graph.total).toBe(2500);
      expect(graph.capped).toBe(true);
      expect(graph.nodes.length).toBe(NODE_CAP);
      expect(graph.nodes.every((node) => node.folder === "Linked")).toBe(true);
    },
  );
});

describe("searching the drawing", () => {
  const ROWS = [
    { path: "a.md", name: "Alpha", folder: "" },
    { path: "b.md", name: "Beta", folder: "" },
    { path: "c.md", name: "alphabet", folder: "" },
  ];

  it("names a note whatever case it was typed in", () => {
    expect(matchesQuery("Alpha", "alp")).toBe(true);
    expect(matchesQuery("Alpha", "  ALPHA ")).toBe(true);
    expect(matchesQuery("Beta", "alp")).toBe(false);
  });

  it("names every note when nothing is typed", () => {
    expect(matchesQuery("Beta", "")).toBe(true);
    expect(matchesQuery("Beta", "   ")).toBe(true);
  });

  it("counts the notes a search names", () => {
    const graph = folderGraphOf(ROWS, []);
    expect(countMatches(graph.nodes, "alp")).toBe(2);
    expect(countMatches(graph.nodes, "")).toBe(3);
    expect(countMatches(graph.nodes, "nothing")).toBe(0);
  });
});

describe("fileGraphOf", () => {
  const nodes = [
    { path: "a.md", name: "A", folder: "" },
    { path: "Work/b.md", name: "B", folder: "Work" },
    { path: "Work/c.md", name: "C", folder: "Work" },
    { path: "d.md", name: "D", folder: "" },
  ];
  const edges = [
    { from_path: "a.md", to_path: "Work/b.md" },
    { from_path: "Work/c.md", to_path: "a.md" },
    { from_path: "Work/c.md", to_path: "d.md" },
  ];

  it("is the open file and the files one link from it, each in its folder", () => {
    const drawn = fileGraphOf(nodes, edges, "a.md");
    expect(drawn.nodes.map((node) => [node.path, node.folder])).toEqual([
      ["a.md", ""],
      ["Work/b.md", "Work"],
      ["Work/c.md", "Work"],
    ]);
    expect(drawn.capped).toBe(false);
  });

  it("is empty for a file nothing links with, and for no file", () => {
    expect(fileGraphOf(nodes, [], "a.md").nodes).toEqual([]);
    expect(fileGraphOf(nodes, edges, "").nodes).toEqual([]);
  });
});

describe("graphMatches and stepMatch", () => {
  const drawn = folderGraphOf(
    [
      { path: "Work/zeta.md", name: "Zeta", folder: "Work" },
      { path: "alpha.md", name: "Alpha", folder: "" },
      { path: "Work/beta.md", name: "Beta", folder: "Work" },
    ],
    [],
  ).nodes;

  it("lists what the search names among the lit folder, by name", () => {
    expect(graphMatches(drawn, "", null).map((node) => node.name)).toEqual(["Alpha", "Beta", "Zeta"]);
    expect(graphMatches(drawn, "a", "Work").map((node) => node.name)).toEqual(["Beta", "Zeta"]);
    expect(graphMatches(drawn, "", "").map((node) => node.name)).toEqual(["Alpha"]);
  });

  it("steps forward and back, wrapping, from no choice or a stale one", () => {
    const matches = graphMatches(drawn, "", null);
    expect(stepMatch(matches, null, 1)).toBe("alpha.md");
    expect(stepMatch(matches, null, -1)).toBe("Work/zeta.md");
    expect(stepMatch(matches, "Work/zeta.md", 1)).toBe("alpha.md");
    expect(stepMatch(matches, "alpha.md", -1)).toBe("Work/zeta.md");
    expect(stepMatch(matches, "gone.md", 1)).toBe("alpha.md");
    expect(stepMatch([], null, 1)).toBeNull();
  });
});

describe("folderLegend", () => {
  it("gives each top-level folder a chip in its colour, then the top level", () => {
    const drawn = folderGraphOf(
      [
        { path: "Work/a.md", name: "A", folder: "Work" },
        { path: "Home/b.md", name: "B", folder: "Home" },
        { path: "c.md", name: "C", folder: "" },
      ],
      [],
    ).nodes;
    const colors = new Map([
      ["Home", "red"],
      ["Work", "blue"],
    ]);
    expect(folderLegend(drawn, colors, "grey", "Writ")).toEqual([
      { folder: "Home", label: "Home", color: "red" },
      { folder: "Work", label: "Work", color: "blue" },
      { folder: "", label: "Writ", color: "grey" },
    ]);
  });

  it("has no top-level chip when every file is in a folder", () => {
    const drawn = folderGraphOf([{ path: "Work/a.md", name: "A", folder: "Work" }], []).nodes;
    expect(folderLegend(drawn, new Map(), "grey", "Writ").map((entry) => entry.label)).toEqual([
      "Work",
    ]);
  });
});
