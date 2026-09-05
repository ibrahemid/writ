import { describe, it, expect } from "vitest";

import {
  planExternalEdit,
  type ExternalChange,
  type ExternalEditAction,
} from "../external-edit";
import fixture from "./external-change-table.json";

// One row per situation a `buffer:external` event can put a tab in, in a file
// rather than in a `describe`, so a row added to the policy is added to the
// table in one place and the count below catches the one that was not.
//
// The whole decision is here. It turns on whether the document holds text no
// file has, which is the editor's answer and nothing else's (ADR-033 §6), so
// there is no second table in Rust for it to drift from.
interface FixtureRow {
  name: string;
  inputs: { known: boolean; change: ExternalChange; hasUnsaved: boolean };
  action: ExternalEditAction;
}

const rows = fixture.rows as FixtureRow[];

describe("the external-change table", () => {
  it("holds a row for every situation", () => {
    expect(rows.length).toBe(12);
  });

  it.each(rows)("routes $name to its action", (row) => {
    expect(planExternalEdit(row.inputs)).toBe(row.action);
  });

  it("never replaces unsaved text without asking", () => {
    // The one row the whole feature exists for. Nothing may turn it into a
    // reload, whatever else changes.
    expect(
      planExternalEdit({ known: true, change: "modified", hasUnsaved: true }),
    ).toBe("prompt");
  });

  it("ignores a file this window holds no tab for", () => {
    // `known` is whether this window holds a tab for the file at all. Nothing
    // else about the event matters when it does not.
    for (const row of rows.filter((r) => !r.inputs.known)) {
      expect(planExternalEdit(row.inputs), row.name).toBe("ignore");
    }
  });

  it("reaches every action the route can take", () => {
    const actions = new Set(rows.map((row) => row.action));
    expect([...actions].sort()).toEqual([
      "follow",
      "ignore",
      "mark-removed",
      "prompt",
      "reload",
    ]);
  });
});
