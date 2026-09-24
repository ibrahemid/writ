import { describe, expect, it } from "vitest";
import { NotesIndex } from "../backend/notes-index";
import { VirtualFolder } from "../backend/vfs";

const ROOT = "/Users/you/Notes";
const at = (relative: string) => `${ROOT}/${relative}`;

function indexOf(files: Record<string, string>) {
  const folder = new VirtualFolder(files);
  return { folder, index: new NotesIndex(folder, ROOT) };
}

const FILES = {
  "Plan.md": "---\ntags: [project/garden]\nplot: 14b\n---\n# Plan\n\n## Beds\nSee [[Seed order]] and [[Seed order#Bulbs|bulbs]].\n",
  "Garden/Seed order.md": "# Seed order\n\n## Bulbs\nFor the [Plan](../Plan.md). #project\n",
  "a/Note.md": "one #shared\n",
  "b/Note.md": "two #shared\n",
  "Reader.md": "Which [[Note]]? And [[Missing]].\n",
  "list.txt": "a text file with #todo and [[Plan]]\n",
};

describe("NotesIndex", () => {
  it("reads one note's links, properties, tags and headings", () => {
    const { index } = indexOf(FILES);
    const facts = index.getFacts(at("Plan.md"));
    expect(facts.links.map((l) => [l.to_target, l.to_path, l.kind, l.line])).toEqual([
      ["Seed order", at("Garden/Seed order.md"), "wikilink", 8],
      ["Seed order", at("Garden/Seed order.md"), "wikilink", 8],
    ]);
    expect(facts.properties).toEqual([
      { key: "tags", value_json: '["project/garden"]' },
      { key: "plot", value_json: '"14b"' },
    ]);
    expect(facts.tags).toEqual([{ tag: "project/garden", line: 2 }]);
    expect(facts.headings.map((h) => h.slug)).toEqual(["plan", "beds"]);
    expect(index.getFacts(at("Nope.md"))).toEqual({ links: [], properties: [], tags: [], headings: [] });
  });

  it("resolves a written target with its heading line, and leaves a tie ambiguous", () => {
    const { index } = indexOf(FILES);
    expect(index.resolveLink(at("Plan.md"), "Seed order#Bulbs")).toEqual({
      status: "resolved",
      path: at("Garden/Seed order.md"),
      candidates: [],
      heading_line: 3,
    });
    expect(index.resolveLink(at("Reader.md"), "Note")).toEqual({
      status: "ambiguous",
      path: null,
      candidates: [at("a/Note.md"), at("b/Note.md")],
      heading_line: null,
    });
    expect(index.resolveLink(at("Reader.md"), "Missing").status).toBe("missing");
    expect(index.findHeadingLine(at("Plan.md"), "Beds")).toBe(7);
  });

  it("lists backlinks with their sentence, ambiguous ones flagged with the other candidates", () => {
    const { index } = indexOf(FILES);
    const seed = index.listBacklinks(at("Garden/Seed order.md"));
    expect(seed.map((b) => [b.from_name, b.alias, b.certainty])).toEqual([
      ["Plan", null, "resolved"],
      ["Plan", "bulbs", "resolved"],
    ]);
    expect(seed[0].context).toBe("See [[Seed order]] and [[Seed order#Bulbs|bulbs]].");
    const note = index.listBacklinks(at("a/Note.md"));
    expect(note).toHaveLength(1);
    expect(note[0]).toMatchObject({ from_path: at("Reader.md"), certainty: "ambiguous", candidates: [at("b/Note.md")] });
    expect(index.countLinksTo(at("Plan.md"))).toBe(2);
  });

  it("draws note files as nodes and resolved links between them as edges", () => {
    const { index } = indexOf(FILES);
    const graph = index.buildGraph();
    expect(graph.nodes.map((n) => n.path)).not.toContain(at("list.txt"));
    expect(graph.nodes.find((n) => n.path === at("Garden/Seed order.md"))).toEqual({
      path: at("Garden/Seed order.md"),
      name: "Seed order",
      folder: "Garden",
    });
    expect(graph.edges).toEqual([
      { from_path: at("Garden/Seed order.md"), to_path: at("Plan.md"), count: 1 },
      { from_path: at("Plan.md"), to_path: at("Garden/Seed order.md"), count: 2 },
    ]);
  });

  it("counts tags by note and filters a tag's whole family", () => {
    const { index } = indexOf(FILES);
    expect(index.listAllTags()).toEqual([
      { tag: "shared", count: 2 },
      { tag: "project", count: 1 },
      { tag: "project/garden", count: 1 },
      { tag: "todo", count: 1 },
    ]);
    expect(index.listPathsForTag("Project")).toEqual([at("Garden/Seed order.md"), at("Plan.md")]);
    expect(index.listPathsForTag("project/garden")).toEqual([at("Plan.md")]);
  });

  it("offers names and folders for completion and lists a folder's notes", () => {
    const { index } = indexOf(FILES);
    expect(index.listNameCandidates("seed")[0]).toEqual({ path: at("Garden/Seed order.md"), name: "Seed order.md", folder: "Garden" });
    expect(index.listNameCandidates("  ")).toEqual([]);
    expect(index.listFolderCandidates("gar")).toEqual([{ folder: "Garden", notes: 1 }]);
    expect(index.listPathsInFolder("Garden/")).toEqual([at("Garden/Seed order.md")]);
    expect(index.listPathsInFolder("Gar")).toEqual([]);
  });

  it("answers from the folder as it is now", () => {
    const { folder, index } = indexOf(FILES);
    folder.write(at("New.md"), "Links to [[Plan]]");
    expect(index.countLinksTo(at("Plan.md"))).toBe(3);
  });
});
