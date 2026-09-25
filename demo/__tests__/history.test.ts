import { describe, expect, it } from "vitest";
import { NoteHistory, VersionMissingError } from "../backend/history";
import { SEED_VERSIONS, computeSeededVersionAgeMs } from "../backend/seed";

function clock() {
  let now = 1_000_000;
  return { now: () => now, advance: (ms: number) => (now += ms) };
}

describe("NoteHistory", () => {
  it("keeps what the first save replaced, then the save itself", () => {
    const time = clock();
    const history = new NoteHistory(time.now);
    history.captureWrite("/n/a.md", "original", "first edit", "editor");
    expect(history.listVersions("/n/a.md").map((v) => history.getEntry(v.id).text)).toEqual(["first edit", "original"]);
  });

  it("folds a run of editor saves inside the merge window into one version", () => {
    const time = clock();
    const history = new NoteHistory(time.now);
    history.captureWrite("/n/a.md", "original", "one", "editor");
    time.advance(3_000);
    history.captureWrite("/n/a.md", "one", "one two", "editor");
    expect(history.listVersions("/n/a.md").map((v) => history.getEntry(v.id).text)).toEqual(["one two", "original"]);
    time.advance(10_000);
    history.captureWrite("/n/a.md", "one two", "one two three", "editor");
    expect(history.listVersions("/n/a.md")).toHaveLength(3);
  });

  it("never merges a restore or a rename rewrite into a run", () => {
    const time = clock();
    const history = new NoteHistory(time.now);
    history.captureWrite("/n/a.md", "original", "edit", "editor");
    time.advance(1_000);
    history.captureWrite("/n/a.md", "edit", "original", "restore");
    expect(history.listVersions("/n/a.md").map((v) => history.getEntry(v.id).text)).toEqual(["original", "edit", "original"]);
  });

  it("follows a renamed note and refuses a version it does not hold", () => {
    const history = new NoteHistory(clock().now);
    history.captureWrite("/n/a.md", null, "text", "editor");
    history.follow("/n/a.md", "/n/b.md");
    expect(history.listVersions("/n/a.md")).toEqual([]);
    expect(history.listVersions("/n/b.md")).toHaveLength(1);
    expect(() => history.getEntry(99)).toThrow(VersionMissingError);
  });

  it("lists seeded versions at the times they were given, beside later saves", () => {
    const time = clock();
    const history = new NoteHistory(time.now);
    history.seedVersion("/n/a.md", "oldest", 100);
    history.seedVersion("/n/a.md", "newer", 500);
    expect(history.listVersions("/n/a.md").map((v) => [history.getEntry(v.id).text, v.at_ms])).toEqual([
      ["newer", 500],
      ["oldest", 100],
    ]);
    history.captureWrite("/n/a.md", "current", "edited", "editor");
    expect(history.listVersions("/n/a.md").map((v) => history.getEntry(v.id).text)).toEqual(["edited", "current", "newer", "oldest"]);
  });
});

describe("the seeded newsletter history", () => {
  const HOUR = 3_600_000;

  it("spaces versions as seed-history.sh does: four days, two days less three hours, 2 h 17 min", () => {
    expect([0, 1, 2].map((index) => computeSeededVersionAgeMs(index, 3))).toEqual([96 * HOUR, 45 * HOUR, 2 * HOUR + 17 * 60_000]);
  });

  it("holds the three capture fixtures, oldest first", () => {
    expect(SEED_VERSIONS).toHaveLength(3);
    expect(SEED_VERSIONS[0].text).toContain("Something about the allotment");
    expect(SEED_VERSIONS[2].text).toContain("## Still to write");
    expect(SEED_VERSIONS.map((version) => version.ageMs)).toEqual([...SEED_VERSIONS.map((version) => version.ageMs)].sort((a, b) => b - a));
  });
});
