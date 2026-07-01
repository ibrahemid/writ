import { describe, it, expect } from "vitest";
import { basename, dirname } from "../../lib/path";

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
