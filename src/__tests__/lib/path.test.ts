import { describe, it, expect } from "vitest";
import { basename, dirname, joinPath, pathKey } from "../../lib/path";

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
