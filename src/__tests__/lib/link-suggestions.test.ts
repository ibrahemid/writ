import { describe, it, expect } from "vitest";
import { emptyQueryLinks, isLinkable } from "../../lib/link-suggestions";

const ROOT = "/Users/me/Writ";

describe("emptyQueryLinks", () => {
  it("offers recent files before nearby ones, each once, without the file being edited", () => {
    const links = emptyQueryLinks(
      [`${ROOT}/plan.md`, `${ROOT}/work/standup.md`, `${ROOT}/work/today.md`],
      [`${ROOT}/work/today.md`, `${ROOT}/work/retro.md`, `${ROOT}/work/deep/notes.txt`],
      `${ROOT}/work/today.md`,
      ROOT,
    );
    expect(links).toEqual([
      { path: `${ROOT}/plan.md`, name: "plan", folder: "" },
      { path: `${ROOT}/work/standup.md`, name: "standup", folder: "work" },
      { path: `${ROOT}/work/retro.md`, name: "retro", folder: "work" },
      { path: `${ROOT}/work/deep/notes.txt`, name: "notes", folder: "work/deep" },
    ]);
  });

  it("leaves out files a link cannot name", () => {
    const links = emptyQueryLinks(
      ["/elsewhere/outside.md", `${ROOT}/script.js`, `${ROOT}/keep.md`],
      [],
      null,
      ROOT,
    );
    expect(links.map((link) => link.name)).toEqual(["keep"]);
  });

  it("stops at the limit", () => {
    const many = Array.from({ length: 30 }, (_, i) => `${ROOT}/f${i}.md`);
    expect(emptyQueryLinks(many, [], null, ROOT, 5)).toHaveLength(5);
  });

  it("offers nothing without a notes folder", () => {
    expect(emptyQueryLinks([`${ROOT}/a.md`], [], null, null)).toEqual([]);
  });
});

describe("isLinkable", () => {
  it("takes text files inside the notes folder", () => {
    expect(isLinkable(`${ROOT}/a.markdown`, ROOT)).toBe(true);
    expect(isLinkable(`${ROOT}/a.png`, ROOT)).toBe(false);
    expect(isLinkable(ROOT, ROOT)).toBe(false);
  });
});
