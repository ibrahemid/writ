import { describe, it, expect } from "vitest";
import { buildTagTree, type TagNode } from "../../lib/tag-tree";
import type { TagCount } from "../../stores/global/note-facts";

function tags(...rows: [string, number][]): TagCount[] {
  return rows.map(([tag, count]) => ({ tag, count }));
}

/** The tree as `tag:count` lines, children indented, so a shape reads at a glance. */
function outline(nodes: TagNode[], depth = 0): string[] {
  return nodes.flatMap((node) => [
    `${"  ".repeat(depth)}${node.name}=${node.tag}:${node.count}`,
    ...outline(node.children, depth + 1),
  ]);
}

describe("buildTagTree", () => {
  it("builds nothing from nothing", () => {
    expect(buildTagTree([])).toEqual([]);
  });

  it("keeps flat tags as one level of leaves", () => {
    expect(outline(buildTagTree(tags(["idea", 3], ["draft", 1])))).toEqual([
      "idea=idea:3",
      "draft=draft:1",
    ]);
  });

  it("puts a nested tag under its parent, the parent keeping its own count", () => {
    expect(
      outline(buildTagTree(tags(["project", 1], ["project/alpha", 1], ["project/beta", 1]))),
    ).toEqual(["project=project:1", "  alpha=project/alpha:1", "  beta=project/beta:1"]);
  });

  it("makes a parent nobody uses directly, counted 0, where its first child appeared", () => {
    expect(
      outline(buildTagTree(tags(["idea", 3], ["project/alpha", 2], ["draft", 1], ["project/beta", 1]))),
    ).toEqual([
      "idea=idea:3",
      "project=project:0",
      "  alpha=project/alpha:2",
      "  beta=project/beta:1",
      "draft=draft:1",
    ]);
  });

  it("nests three levels", () => {
    expect(
      outline(buildTagTree(tags(["a/b/c", 1], ["a/b", 2], ["a", 3], ["a/b/d", 1]))),
    ).toEqual(["a=a:3", "  b=a/b:2", "    c=a/b/c:1", "    d=a/b/d:1"]);
  });

  it("folds case so two spellings share one parent", () => {
    expect(outline(buildTagTree(tags(["Project/Alpha", 1], ["project/beta", 1])))).toEqual([
      "project=project:0",
      "  alpha=project/alpha:1",
      "  beta=project/beta:1",
    ]);
  });

  it("drops a trailing slash", () => {
    expect(outline(buildTagTree(tags(["project/", 2])))).toEqual(["project=project:2"]);
  });

  it("drops an empty middle segment", () => {
    expect(outline(buildTagTree(tags(["a//b", 1])))).toEqual(["a=a:0", "  b=a/b:1"]);
  });

  it("adds the counts of one tag spelled twice", () => {
    expect(outline(buildTagTree(tags(["Idea", 2], ["idea", 3])))).toEqual(["idea=idea:5"]);
  });

  it("drops an empty or whitespace-only tag", () => {
    expect(buildTagTree(tags(["", 1], ["   ", 2], ["/", 3], ["idea", 1]))).toHaveLength(1);
  });

  it("keeps the order the index hands over at every level", () => {
    expect(
      outline(buildTagTree(tags(["z", 5], ["m/b", 4], ["a", 3], ["m/a", 2], ["m", 1]))),
    ).toEqual(["z=z:5", "m=m:1", "  b=m/b:4", "  a=m/a:2", "a=a:3"]);
  });
});
