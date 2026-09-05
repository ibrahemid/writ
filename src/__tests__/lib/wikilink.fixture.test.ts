import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { EditorState } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";

import { findLinkTargets } from "../../editor/link-layer";
import { wikilinkName } from "../../lib/wikilink";

// The other half is crates/writ-core/tests/wikilink_targets.rs, reading this
// same file. writ-core decides what the index stores and what a link resolves
// to; the editor decides what is painted and what a click follows. A link the
// editor offers to follow and the index does not hold is a link that goes
// nowhere, so the two agreeing is the whole contract.
interface FixtureCase {
  name: string;
  text: string;
  targets: string[];
  names: string[];
  // What the editor finds instead, for a case where the difference is on
  // purpose. `why` says what for.
  editorTargets?: string[];
  editorNames?: string[];
  why?: string;
}

const cases: FixtureCase[] = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../crates/writ-core/tests/fixtures/wikilink-targets.json"),
    "utf8",
  ),
).cases;

/** The raw text inside each `[[…]]` the editor finds in `text`. */
function targets(text: string): string[] {
  const state = EditorState.create({
    doc: text,
    extensions: [markdown({ base: markdownLanguage })],
  });
  return findLinkTargets(state, 0, state.doc.length)
    .filter((range) => range.kind === "wikilink")
    .map((range) => state.doc.sliceString(range.from, range.to));
}

describe("the wikilink fixture shared with writ-core", () => {
  it("carries the cases the contract needs", () => {
    expect(cases.length).toBeGreaterThan(20);
    const named = cases.map((one) => one.name);
    for (const wanted of ["fenced", "inline code", "inner close", "extension"]) {
      expect(named).toContain(wanted);
    }
  });

  it("says what every deliberate difference is for", () => {
    for (const one of cases) {
      if (one.editorTargets === undefined) continue;
      expect(one.why, `case ${one.name}`).toBeTruthy();
    }
  });

  it.each(cases.map((one) => [one.name, one] as const))(
    "finds the same links as writ-core in %s",
    (_name, one) => {
      // A target that names nothing names no note, which is what writ-core's
      // scanner drops rather than storing.
      const found = targets(one.text).filter((target) => wikilinkName(target) !== "");
      expect(found).toEqual(one.editorTargets ?? one.targets);
      expect(found.map(wikilinkName)).toEqual(one.editorNames ?? one.names);
    },
  );
});
