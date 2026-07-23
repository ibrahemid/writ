import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import {
  computeFixChanges,
  spellingField,
  setSpellingLints,
  spellingEntries,
  type SpellingEntry,
} from "../../editor/spelling";
import type { SpellingLint } from "../../types/spelling";

function entry(partial: Partial<SpellingEntry>): SpellingEntry {
  return {
    from: 0,
    to: 3,
    word: "teh",
    message: "",
    kind: "Spelling",
    suggestions: ["the"],
    confident: true,
    ...partial,
  };
}

// A doc reader backed by an array of [from, to, text] segments.
function readerFrom(segments: Array<[number, number, string]>) {
  return (from: number, to: number) => {
    const hit = segments.find((s) => s[0] === from && s[1] === to);
    return hit ? hit[2] : "";
  };
}

describe("computeFixChanges", () => {
  it("emits a change for a confident entry whose text still matches", () => {
    const e = entry({ from: 4, to: 7, word: "teh", suggestions: ["the"] });
    const changes = computeFixChanges([e], readerFrom([[4, 7, "teh"]]));
    expect(changes).toEqual([{ from: 4, to: 7, insert: "the" }]);
  });

  it("drops an entry whose current text no longer equals the flagged word", () => {
    // The user edited over the range; slice returns something else.
    const e = entry({ from: 4, to: 7, word: "teh", suggestions: ["the"] });
    const changes = computeFixChanges([e], readerFrom([[4, 7, "cat"]]));
    expect(changes).toEqual([]);
  });

  it("drops a non-confident entry", () => {
    const e = entry({ confident: false });
    const changes = computeFixChanges([e], readerFrom([[0, 3, "teh"]]));
    expect(changes).toEqual([]);
  });

  it("drops an entry with no suggestion", () => {
    const e = entry({ suggestions: [] });
    const changes = computeFixChanges([e], readerFrom([[0, 3, "teh"]]));
    expect(changes).toEqual([]);
  });

  it("honors the accept filter", () => {
    const a = entry({ from: 0, to: 3, word: "teh", suggestions: ["the"] });
    const b = entry({ from: 4, to: 8, word: "wrold", suggestions: ["world"] });
    const reader = readerFrom([
      [0, 3, "teh"],
      [4, 8, "wrold"],
    ]);
    const changes = computeFixChanges([a, b], reader, (e) => e.word === "wrold");
    expect(changes).toEqual([{ from: 4, to: 8, insert: "world" }]);
  });

  it("uses the first suggestion as the replacement", () => {
    const e = entry({ from: 0, to: 3, word: "teh", suggestions: ["the", "tech"] });
    const changes = computeFixChanges([e], readerFrom([[0, 3, "teh"]]));
    expect(changes[0].insert).toBe("the");
  });
});

describe("spellingField", () => {
  function withLints(doc: string, lints: SpellingLint[]) {
    const s0 = EditorState.create({ doc, extensions: [spellingField] });
    return s0.update({ effects: setSpellingLints.of(lints) }).state;
  }

  const heloLint: SpellingLint = {
    fromUtf16: 0,
    toUtf16: 4,
    kind: "Spelling",
    message: "misspelling",
    suggestions: ["hello"],
    confident: true,
  };

  it("round-trips the custom spelling spec through Decoration.mark", () => {
    const entries = spellingEntries(withLints("helo wrld", [heloLint]));
    expect(entries).toHaveLength(1);
    expect(entries[0].word).toBe("helo");
    expect(entries[0].suggestions).toEqual(["hello"]);
    expect(entries[0].confident).toBe(true);
  });

  it("remaps entry offsets through an edit before the range", () => {
    const state = withLints("helo wrld", [heloLint]).update({
      changes: { from: 0, to: 0, insert: "XX " },
    }).state;
    const entries = spellingEntries(state);
    expect(entries[0].from).toBe(3);
    expect(entries[0].word).toBe("helo");
  });

  it("drops a decoration once its range is replaced (no stale flash)", () => {
    const state = withLints("helo wrld", [heloLint]).update({
      changes: { from: 0, to: 4, insert: "hello" },
    }).state;
    expect(spellingEntries(state)).toHaveLength(0);
  });

  it("computeFixChanges drops an entry mutated in place", () => {
    const state = withLints("helo wrld", [heloLint]).update({
      changes: { from: 1, to: 2, insert: "a" },
    }).state;
    const entries = spellingEntries(state);
    const changes = computeFixChanges(entries, (f, t) => state.doc.sliceString(f, t));
    expect(changes).toEqual([]);
  });
});
