import { describe, it, expect } from "vitest";
import {
  findByPrefix,
  flattenTree,
  statusLine,
  type TreeRow,
  type TreeSource,
} from "../../components/Sidebar/tree-rows";
import type { WorkspaceEntry } from "../../types/workspace";

function file(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: false, conflict_copy: null };
}

function dir(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: true, conflict_copy: null };
}

interface Fixture {
  listed: Record<string, WorkspaceEntry[]>;
  errors?: Record<string, string>;
  unreadable?: Record<string, number>;
  open?: string[];
}

function source(f: Fixture): TreeSource {
  const open = new Set(f.open ?? []);
  return {
    entriesFor: (path) => f.listed[path],
    errorFor: (path) => f.errors?.[path] ?? null,
    unreadableFor: (path) => f.unreadable?.[path] ?? 0,
    isExpanded: (path) => open.has(path),
  };
}

function shape(rows: TreeRow[]): string[] {
  return rows.map((row) =>
    row.kind === "entry"
      ? `${"  ".repeat(row.level - 1)}${row.entry.name}${row.entry.is_dir ? (row.expanded ? "/-" : "/+") : ""}`
      : `${"  ".repeat(row.level - 1)}[${statusLine(row)}]`,
  );
}

const ROOT = "/w";
const LISTED = {
  "/w": [dir("/w/docs"), dir("/w/src"), file("/w/README.md")],
  "/w/docs": [file("/w/docs/guide.md")],
  "/w/src": [dir("/w/src/lib"), file("/w/src/main.rs")],
  "/w/src/lib": [file("/w/src/lib/mod.rs")],
};

describe("flattenTree", () => {
  it("draws a folder's entries straight after it while it is open, and nothing while it is folded", () => {
    expect(shape(flattenTree(ROOT, source({ listed: LISTED }), null))).toEqual([
      "docs/+",
      "src/+",
      "README.md",
    ]);
    expect(
      shape(flattenTree(ROOT, source({ listed: LISTED, open: ["/w/src", "/w/src/lib"] }), null)),
    ).toEqual(["docs/+", "src/-", "  lib/-", "    mod.rs", "  main.rs", "README.md"]);
  });

  it("keeps a folder open under a folded parent, and draws it again when the parent opens (NV-18)", () => {
    const folded = flattenTree(ROOT, source({ listed: LISTED, open: ["/w/src/lib"] }), null);
    expect(shape(folded)).toEqual(["docs/+", "src/+", "README.md"]);
    const unfolded = flattenTree(
      ROOT,
      source({ listed: LISTED, open: ["/w/src", "/w/src/lib"] }),
      null,
    );
    expect(shape(unfolded)).toContain("    mod.rs");
  });

  it("says where each row sits among its siblings, for a tree with no groups", () => {
    const rows = flattenTree(ROOT, source({ listed: LISTED, open: ["/w/src"] }), null);
    const placed = rows.map((row) =>
      row.kind === "entry" ? [row.entry.name, row.level, row.posinset, row.setsize, row.parent] : null,
    );
    expect(placed).toEqual([
      ["docs", 1, 1, 3, "/w"],
      ["src", 1, 2, 3, "/w"],
      ["lib", 2, 1, 2, "/w/src"],
      ["main.rs", 2, 2, 2, "/w/src"],
      ["README.md", 1, 3, 3, "/w"],
    ]);
    expect(rows.map((row) => row.index)).toEqual([0, 1, 2, 3, 4]);
  });

  it("opens every folder that holds a tagged note and leaves the rest out", () => {
    const tagged = new Set(["/w/src/lib/mod.rs"]);
    expect(shape(flattenTree(ROOT, source({ listed: LISTED }), tagged))).toEqual([
      "src/-",
      "  lib/-",
      "    mod.rs",
    ]);
  });

  it("says a folder could not be read, is empty, or holds entries it could not read (NV-16)", () => {
    const rows = flattenTree(
      ROOT,
      source({
        listed: {
          "/w": [dir("/w/locked"), dir("/w/empty"), dir("/w/mixed")],
          "/w/empty": [],
          "/w/mixed": [file("/w/mixed/a.md")],
        },
        errors: { "/w/locked": "Could not read this folder." },
        unreadable: { "/w/mixed": 2, "/w": 1 },
        open: ["/w/locked", "/w/empty", "/w/mixed"],
      }),
      null,
    );
    expect(shape(rows)).toEqual([
      "locked/-",
      "  [Could not read this folder.]",
      "empty/-",
      "  [Empty folder]",
      "mixed/-",
      "  a.md",
      "  [Could not read 2 items]",
      "[Could not read 1 item]",
    ]);
    const failed = rows[1];
    expect(failed.kind === "status" && failed.dir).toBe("/w/locked");
    expect(failed.parent).toBe("/w/locked");
  });

  it("draws nothing under a folder still being read for the first time", () => {
    const rows = flattenTree(
      ROOT,
      source({ listed: { "/w": [dir("/w/slow")] }, open: ["/w/slow"] }),
      null,
    );
    expect(shape(rows)).toEqual(["slow/-"]);
  });

  it("lays out a folder of 20,000 entries in one pass", () => {
    const entries = Array.from({ length: 20_000 }, (_, i) =>
      file(`/w/big/f${String(i).padStart(5, "0")}.js`),
    );
    const rows = flattenTree(
      ROOT,
      source({ listed: { "/w": [dir("/w/big")], "/w/big": entries }, open: ["/w/big"] }),
      null,
    );
    expect(rows).toHaveLength(20_001);
    const last = rows[20_000];
    expect(last.kind === "entry" && [last.posinset, last.setsize]).toEqual([20_000, 20_000]);
  });
});

describe("findByPrefix", () => {
  const rows = flattenTree(ROOT, source({ listed: LISTED, open: ["/w/src"] }), null);

  it("finds the next row whose name starts with the typed letters, wrapping to the top", () => {
    expect(findByPrefix(rows, 0, "m")).toBe(3);
    expect(findByPrefix(rows, 3, "d")).toBe(0);
    expect(findByPrefix(rows, 0, "READ")).toBe(4);
    expect(findByPrefix(rows, 0, "zz")).toBeNull();
  });
});
