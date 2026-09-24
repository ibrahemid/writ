import { describe, expect, it } from "vitest";
import { NoteHistory, VersionMissingError } from "../backend/history";

function clock() {
  let now = 1_000_000;
  return { now: () => now, advance: (ms: number) => (now += ms) };
}

describe("NoteHistory", () => {
  it("keeps what the first save replaced, then the save itself", () => {
    const time = clock();
    const history = new NoteHistory(time.now);
    history.captureWrite("/n/a.md", "original", "first edit", "editor");
    expect(history.versions("/n/a.md").map((v) => history.entry(v.id).text)).toEqual(["first edit", "original"]);
  });

  it("folds a run of editor saves inside the merge window into one version", () => {
    const time = clock();
    const history = new NoteHistory(time.now);
    history.captureWrite("/n/a.md", "original", "one", "editor");
    time.advance(3_000);
    history.captureWrite("/n/a.md", "one", "one two", "editor");
    expect(history.versions("/n/a.md").map((v) => history.entry(v.id).text)).toEqual(["one two", "original"]);
    time.advance(10_000);
    history.captureWrite("/n/a.md", "one two", "one two three", "editor");
    expect(history.versions("/n/a.md")).toHaveLength(3);
  });

  it("never merges a restore or a rename rewrite into a run", () => {
    const time = clock();
    const history = new NoteHistory(time.now);
    history.captureWrite("/n/a.md", "original", "edit", "editor");
    time.advance(1_000);
    history.captureWrite("/n/a.md", "edit", "original", "restore");
    expect(history.versions("/n/a.md").map((v) => history.entry(v.id).text)).toEqual(["original", "edit", "original"]);
  });

  it("follows a renamed note and refuses a version it does not hold", () => {
    const history = new NoteHistory(clock().now);
    history.captureWrite("/n/a.md", null, "text", "editor");
    history.follow("/n/a.md", "/n/b.md");
    expect(history.versions("/n/a.md")).toEqual([]);
    expect(history.versions("/n/b.md")).toHaveLength(1);
    expect(() => history.entry(99)).toThrow(VersionMissingError);
  });
});
