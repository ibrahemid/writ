import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import {
  planExternalEdit,
  type ExternalChange,
  type ExternalEditAction,
} from "../external-edit";

// The other half is crates/writ-core/tests/external_change_table.rs, reading
// this same file. Rust cannot answer the question alone — whether a document
// is unsaved is the editor's answer and nothing else's (ADR-033 §6) — so the
// two tables are pinned against each other here instead of one calling the
// other.
interface FixtureRow {
  name: string;
  inputs: { known: boolean; change: ExternalChange; hasUnsaved: boolean };
  action: ExternalEditAction;
  decidedBy: "shared" | "frontend";
  rust?: { dirty: boolean; changed: boolean; removed: boolean };
  plan?: "replace_quietly" | "ask" | "ignore";
}

const fixture = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../crates/writ-core/tests/fixtures/external-change-table.json"),
    "utf8",
  ),
) as { planActions: Record<string, ExternalEditAction[]>; rows: FixtureRow[] };

describe("the external-change table shared with writ-core", () => {
  it("holds every row both halves read", () => {
    expect(fixture.rows.length).toBe(12);
  });

  it.each(fixture.rows)("routes $name to its action", (row) => {
    expect(planExternalEdit(row.inputs)).toBe(row.action);
  });

  it.each(fixture.rows.filter((row) => row.decidedBy === "shared"))(
    "answers $name the way the policy plans it",
    (row) => {
      // The plan is written from the same situation the row describes, so the
      // inputs have to line up before the actions can be compared at all.
      expect(row.rust).toEqual({
        dirty: row.inputs.hasUnsaved,
        changed: row.inputs.change === "modified",
        removed: row.inputs.change === "removed",
      });
      expect(fixture.planActions[row.plan!]).toContain(row.action);
    },
  );

  it("reaches every action the route can take", () => {
    const actions = new Set(fixture.rows.map((row) => row.action));
    expect([...actions].sort()).toEqual([
      "follow",
      "ignore",
      "mark-removed",
      "prompt",
      "reload",
    ]);
  });

  it("leaves only the rows Rust cannot see to the editor alone", () => {
    // `known` is whether this window holds a tab for the file, which the
    // backend has no way to answer. Anything else being frontend-only would
    // be a decision that has quietly left the shared table.
    for (const row of fixture.rows) {
      expect(row.decidedBy).toBe(row.inputs.known ? "shared" : "frontend");
    }
  });
});
