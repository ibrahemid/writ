import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { ChangeSet, Text } from "@codemirror/state";
import { changesBetween } from "../../editor/change-diff";

interface FixtureCase {
  name: string;
  base: string;
  specs: { from: number; to?: number; insert?: string }[];
  changes: unknown;
  result: string;
}

const cases: FixtureCase[] = JSON.parse(
  readFileSync(resolve(__dirname, "../../../crates/writ-core/tests/fixtures/change-sets.json"), "utf8"),
).cases;

const textOf = (value: string) => Text.of(value.split("\n"));

describe("the change-set fixture shared with writ-core", () => {
  it("carries the cases the contract needs", () => {
    expect(cases.length).toBeGreaterThanOrEqual(10);
    const names = cases.map((one) => one.name);
    expect(names).toContain("emoji kept around an edit");
    expect(names).toContain("type into an empty document");
  });

  it.each(cases)("$name is the JSON CodeMirror writes for those edits", (one) => {
    expect(ChangeSet.of(one.specs, textOf(one.base).length).toJSON()).toEqual(one.changes);
  });

  it.each(cases)("$name gives the document writ-core gives", (one) => {
    const applied = ChangeSet.fromJSON(one.changes).apply(textOf(one.base));
    expect(applied.toString()).toBe(one.result);
  });

  it.each(cases)("$name diffed from its two documents gives the same result", (one) => {
    const base = textOf(one.base);
    const doc = ChangeSet.fromJSON(one.changes).apply(base);
    expect(changesBetween(base, doc).apply(base).toString()).toBe(one.result);
  });
});
