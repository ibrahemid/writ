import { describe, it, expect } from "vitest";
import { basename, dirname, joinPath, pathKey, resolveWithinRoot } from "../../lib/path";

describe("basename", () => {
  it("returns the last segment of a posix path", () => {
    expect(basename("/home/user/notes.md")).toBe("notes.md");
    expect(basename("/home/user/project")).toBe("project");
  });

  it("returns the last segment of a windows path", () => {
    expect(basename("C:\\Users\\me\\notes.md")).toBe("notes.md");
    expect(basename("C:\\Users\\me\\project")).toBe("project");
  });

  it("handles a bare name with no separator", () => {
    expect(basename("notes.md")).toBe("notes.md");
  });

  it("falls back to the whole input on a trailing separator", () => {
    expect(basename("/home/user/")).toBe("/home/user/");
    expect(basename("C:\\Users\\me\\")).toBe("C:\\Users\\me\\");
  });

  it("uses whichever separator appears last (mixed)", () => {
    expect(basename("/home/user\\notes.md")).toBe("notes.md");
  });
});

describe("dirname", () => {
  it("returns the parent of a posix path", () => {
    expect(dirname("/home/user/notes.md")).toBe("/home/user");
    expect(dirname("/home/user/project")).toBe("/home/user");
  });

  it("returns the parent of a windows path", () => {
    expect(dirname("C:\\Users\\me\\notes.md")).toBe("C:\\Users\\me");
  });

  it("returns the input unchanged when there is no parent", () => {
    expect(dirname("notes.md")).toBe("notes.md");
    expect(dirname("/foo")).toBe("/foo");
  });
});

describe("joinPath", () => {
  it("joins a posix root with a forward-slash relative path", () => {
    expect(joinPath("/home/user/repo", "src/main.rs")).toBe("/home/user/repo/src/main.rs");
  });

  it("keeps the windows separator of the root", () => {
    expect(joinPath("C:\\Users\\me\\repo", "src/main.rs")).toBe("C:\\Users\\me\\repo\\src/main.rs");
  });

  it("collapses a trailing root separator and a leading relative separator", () => {
    expect(joinPath("/repo/", "/src/main.rs")).toBe("/repo/src/main.rs");
  });

  it("returns the relative path when the root is empty", () => {
    expect(joinPath("", "src/main.rs")).toBe("src/main.rs");
  });

  it("returns the root when the relative path is empty", () => {
    expect(joinPath("/repo", "")).toBe("/repo");
  });
});

describe("pathKey", () => {
  it("matches a windows buffer path against a joined workspace path", () => {
    const fromBuffer = "C:\\Users\\me\\repo\\src\\main.rs";
    const fromWorkspace = joinPath("C:\\Users\\me\\repo", "src/main.rs");
    expect(pathKey(fromWorkspace)).toBe(pathKey(fromBuffer));
  });

  it("leaves a posix path unchanged", () => {
    expect(pathKey("/repo/src/main.rs")).toBe("/repo/src/main.rs");
  });

  it("keeps distinct files distinct", () => {
    expect(pathKey("/repo/src/a.rs")).not.toBe(pathKey("/repo/src/b.rs"));
  });
});

describe("resolveWithinRoot", () => {
  const root = "/home/u/project";
  const base = "/home/u/project/docs";

  it("resolves a relative destination against the file's own folder", () => {
    expect(resolveWithinRoot(root, base, "./notes.md")).toBe("/home/u/project/docs/notes.md");
    expect(resolveWithinRoot(root, base, "notes.md")).toBe("/home/u/project/docs/notes.md");
    expect(resolveWithinRoot(root, base, "sub/notes.md")).toBe(
      "/home/u/project/docs/sub/notes.md",
    );
  });

  it("collapses .. while it stays inside the root", () => {
    expect(resolveWithinRoot(root, base, "../README.md")).toBe("/home/u/project/README.md");
    expect(resolveWithinRoot(root, base, "../src/./main.ts")).toBe("/home/u/project/src/main.ts");
  });

  it("refuses a destination that escapes the root", () => {
    expect(resolveWithinRoot(root, base, "../../secrets.txt")).toBeNull();
    expect(resolveWithinRoot(root, base, "../../../../../../etc/passwd")).toBeNull();
    expect(resolveWithinRoot(root, base, "/etc/passwd")).toBeNull();
    expect(resolveWithinRoot(root, base, "/home/u/.ssh/id_rsa")).toBeNull();
  });

  it("refuses a sibling directory that shares the root's name prefix", () => {
    expect(resolveWithinRoot(root, base, "../../project-backup/notes.md")).toBeNull();
    expect(resolveWithinRoot("/home/u/project", "/home/u", "project-evil/x.md")).toBeNull();
  });

  it("accepts an absolute destination inside the root", () => {
    expect(resolveWithinRoot(root, base, "/home/u/project/src/main.ts")).toBe(
      "/home/u/project/src/main.ts",
    );
  });

  it("drops a fragment and a query before resolving", () => {
    expect(resolveWithinRoot(root, base, "./notes.md#heading")).toBe(
      "/home/u/project/docs/notes.md",
    );
    expect(resolveWithinRoot(root, base, "./notes.md?v=2")).toBe(
      "/home/u/project/docs/notes.md",
    );
    expect(resolveWithinRoot(root, base, "#heading")).toBeNull();
  });

  it("decodes percent-escapes before collapsing, never after", () => {
    expect(resolveWithinRoot(root, base, "./my%20notes.md")).toBe(
      "/home/u/project/docs/my notes.md",
    );
    expect(resolveWithinRoot(root, base, "%2e%2e/%2e%2e/secrets.txt")).toBeNull();
  });

  it("refuses a destination carrying a control character", () => {
    expect(resolveWithinRoot(root, base, "./notes\u0000.md")).toBeNull();
    expect(resolveWithinRoot(root, base, "./notes%00.md")).toBeNull();
  });

  it("refuses an empty destination and a relative base", () => {
    expect(resolveWithinRoot(root, base, "")).toBeNull();
    expect(resolveWithinRoot(root, "docs", "./notes.md")).toBeNull();
    expect(resolveWithinRoot("project", base, "./notes.md")).toBeNull();
  });

  it("resolves windows paths case-insensitively and returns native separators", () => {
    expect(resolveWithinRoot("C:\\work\\proj", "C:\\work\\proj\\docs", "./notes.md")).toBe(
      "C:\\work\\proj\\docs\\notes.md",
    );
    // Containment is decided without regard to case; the path handed back
    // keeps the casing it was resolved from.
    expect(resolveWithinRoot("C:\\work\\proj", "C:\\Work\\Proj\\docs", "../README.md")).toBe(
      "C:\\Work\\Proj\\README.md",
    );
    expect(resolveWithinRoot("C:\\work\\proj", "C:\\work\\proj", "../other/x.md")).toBeNull();
  });
});
