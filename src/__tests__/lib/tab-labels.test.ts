import { describe, it, expect } from "vitest";
import { pathInRoots, tabFolderHints } from "../../lib/tab-labels";

describe("tabFolderHints", () => {
  it("gives a folder only to tabs whose names collide", () => {
    const hints = tabFolderHints([
      { id: "a", source_path: "/n/api/README.md" },
      { id: "b", source_path: "/n/web/README.md" },
      { id: "c", source_path: "/n/plan.md" },
      { id: "d", source_path: null },
    ]);
    expect([...hints.entries()]).toEqual([
      ["a", "api"],
      ["b", "web"],
    ]);
  });

  it("climbs as many folders as it takes to tell them apart", () => {
    const hints = tabFolderHints([
      { id: "a", source_path: "/n/one/docs/index.md" },
      { id: "b", source_path: "/n/two/docs/index.md" },
    ]);
    expect(hints.get("a")).toBe("one/docs");
    expect(hints.get("b")).toBe("two/docs");
  });

  it("treats a .txt and a .md of one stem as the same shown name", () => {
    const hints = tabFolderHints([
      { id: "a", source_path: "/n/a/notes.md" },
      { id: "b", source_path: "/n/b/notes.txt" },
    ]);
    expect(hints.get("a")).toBe("a");
    expect(hints.get("b")).toBe("b");
  });
});

describe("pathInRoots", () => {
  it("reads a path from the first root that holds it", () => {
    expect(pathInRoots("/w/api/README.md", ["/w", "/n"])).toBe("api/README.md");
    expect(pathInRoots("/n/a.md", [null, "/n/"])).toBe("a.md");
  });

  it("keeps a path no root holds whole", () => {
    expect(pathInRoots("/elsewhere/a.md", ["/w"])).toBe("/elsewhere/a.md");
    expect(pathInRoots("/w2/a.md", ["/w"])).toBe("/w2/a.md");
  });

  it("writes a Windows path inside its root with forward slashes", () => {
    expect(pathInRoots("C:\\Users\\me\\Writ\\api\\a.md", ["C:\\Users\\me\\Writ"])).toBe("api/a.md");
  });
});
