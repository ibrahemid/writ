import { describe, it, expect } from "vitest";

import type { ActivityRecord } from "../../services/tauri";
import { FOLD_WINDOW_MS, foldActivity } from "../../lib/activity-rows";

function record(over: Partial<ActivityRecord> = {}): ActivityRecord {
  return {
    at: "2026-09-09T10:30:00.000Z",
    actor: { kind: "client", name: "Claude Code", version: "1.2.3" },
    action: "read_note",
    path: "Ideas/Tessera.md",
    decision: "allow",
    bytes: 412,
    ...over,
  };
}

function at(seconds: number): string {
  return new Date(Date.parse("2026-09-09T10:30:00.000Z") + seconds * 1000).toISOString();
}

describe("foldActivity", () => {
  it("puts the newest call first, whatever order the lines are in", () => {
    const rows = foldActivity([
      record({ at: at(0), path: "a.md" }),
      record({ at: at(120), path: "b.md" }),
      record({ at: at(60), path: "c.md" }),
    ]);
    expect(rows.map((row) => row.record.path)).toEqual(["b.md", "c.md", "a.md"]);
  });

  it("folds reads of one file by one program, answered alike, into one row", () => {
    const rows = foldActivity([record({ at: at(0) }), record({ at: at(10) }), record({ at: at(20) })]);
    expect(rows).toHaveLength(1);
    expect(rows[0].calls).toBe(3);
    expect(rows[0].record.at).toBe(at(20));
  });

  it("adds the count a line already carries", () => {
    const rows = foldActivity([record({ at: at(0) }), record({ at: at(30), count: 7 })]);
    expect(rows).toEqual([{ record: record({ at: at(30), count: 7 }), calls: 8 }]);
  });

  it("starts a new row once a read falls outside the window", () => {
    const window = FOLD_WINDOW_MS / 1000;
    const rows = foldActivity([record({ at: at(0) }), record({ at: at(window) })]);
    expect(rows.map((row) => row.calls)).toEqual([1, 1]);
  });

  it("never folds a write", () => {
    const rows = foldActivity([
      record({ at: at(0), action: "write_note" }),
      record({ at: at(5), action: "write_note" }),
    ]);
    expect(rows.map((row) => row.calls)).toEqual([1, 1]);
  });

  it("keeps apart reads that differ in file, program, tool or answer", () => {
    const rows = foldActivity([
      record({ at: at(0) }),
      record({ at: at(1), path: "Other.md" }),
      record({ at: at(2), actor: { kind: "client", name: "Cursor", version: null } }),
      record({ at: at(3), action: "note_links" }),
      record({ at: at(4), decision: "pending" }),
    ]);
    expect(rows).toHaveLength(5);
  });

  it("folds only rows that sit next to each other in time", () => {
    const rows = foldActivity([
      record({ at: at(0) }),
      record({ at: at(5), path: "Other.md" }),
      record({ at: at(10) }),
    ]);
    expect(rows.map((row) => row.record.path)).toEqual([
      "Ideas/Tessera.md",
      "Other.md",
      "Ideas/Tessera.md",
    ]);
  });
});
