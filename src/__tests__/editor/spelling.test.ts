import { describe, it, expect, vi, afterEach } from "vitest";
import { EditorState, RangeSet, type Extension } from "@codemirror/state";
import { markdown } from "@codemirror/lang-markdown";
import {
  checkRanges,
  computeFixChanges,
  MAX_CHECKED_STRETCHES,
  recheckSpelling,
  sameSpellingRules,
  spellingField,
  spellingPending,
  setSpellingLints,
  setSpellingLintsIn,
  spellingEntries,
  type CheckedRange,
  type SpellingEntry,
} from "../../editor/spelling";
import type { SpellingLint } from "../../types/spelling";

/** `count` one-line paragraphs, each holding the misspelling "teh". */
function paragraphs(count: number): string[] {
  return Array.from({ length: count }, (_, i) => `paragraph ${i} has teh word`);
}

/** A Replace All: one change per `word` in `doc` that starts inside [from, to). */
function replaceAll(doc: string, word: string, insert: string, from = 0, to = doc.length) {
  const changes: Array<{ from: number; to: number; insert: string }> = [];
  for (let at = doc.indexOf(word, from); at >= 0 && at < to; at = doc.indexOf(word, at + word.length)) {
    changes.push({ from: at, to: at + word.length, insert });
  }
  return changes;
}

function lintAt(from: number, to: number): SpellingLint {
  return { fromUtf16: from, toUtf16: to, kind: "Spelling", message: "", suggestions: [], confident: true };
}

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

  it("drops exactly the marks a change touches, across many changes in one transaction", () => {
    let seed = 11;
    const random = () => (seed = (seed * 48271) % 2147483647) / 2147483647;
    const doc = paragraphs(300).join("\n\n");
    const words: Array<[number, number]> = [];
    for (const match of doc.matchAll(/\S+/g)) words.push([match.index, match.index + match[0].length]);
    for (let round = 0; round < 25; round++) {
      const marked = words.filter(() => random() < 0.5);
      const before = withLints(doc, marked.map(([from, to]) => lintAt(from, to)));
      const changes: Array<{ from: number; to: number; insert: string }> = [];
      for (let at = Math.floor(random() * 40); at < doc.length; at += 1 + Math.floor(random() * 80)) {
        const to = Math.min(doc.length, at + Math.floor(random() * 4));
        changes.push({ from: at, to, insert: "xyz".slice(0, Math.floor(random() * 4)) });
        at = to;
      }
      const tr = before.update({ changes });
      const touched: Array<[number, number]> = [];
      tr.changes.iterChangedRanges((_fromA, _toA, fromB, toB) => touched.push([fromB, toB]));
      const expected = before
        .field(spellingField)
        .map(tr.changes)
        .update({ filter: (from, to) => !touched.some(([a, b]) => from < b && to > a) });
      const wanted: string[] = [];
      for (const cursor = expected.iter(); cursor.value; cursor.next()) wanted.push(`${cursor.from}-${cursor.to}`);
      expect(spellingEntries(tr.state).map((entry) => `${entry.from}-${entry.to}`)).toEqual(wanted);
    }
  });

  it("replaces the marks inside each checked stretch, and keeps those outside it or touching its ends", () => {
    const doc = "aaa bbb ccc ddd eee fff";
    const word = (text: string) => lintAt(doc.indexOf(text), doc.indexOf(text) + text.length);
    const state = withLints(doc, ["aaa", "bbb", "ddd", "fff"].map(word));
    const checked: CheckedRange[] = [
      { from: 19, to: 23, lints: [] },
      { from: 3, to: 12, lints: [word("ccc")] },
    ];
    const landed = state.update({ effects: setSpellingLintsIn.of(checked) }).state;
    expect(spellingEntries(landed).map((entry) => `${entry.word}@${entry.from}`)).toEqual([
      "aaa@0",
      "ccc@8",
      "ddd@12",
    ]);
  });

  it("lands the results of many checked stretches in one pass over the marks", () => {
    const parts = paragraphs(200);
    const doc = parts.join("\n\n");
    const state = withLints(
      doc,
      replaceAll(doc, "teh", "teh").map(({ from, to }) => lintAt(from, to)),
    );
    const checked: CheckedRange[] = [];
    let start = 0;
    for (const part of parts) {
      const at = start + part.indexOf("word");
      checked.push({ from: start, to: start + part.length, lints: [lintAt(at, at + 4)] });
      start += part.length + 2;
    }
    const update = vi.spyOn(RangeSet.prototype, "update");
    const landed = state.update({ effects: setSpellingLintsIn.of(checked) }).state;
    expect(update).toHaveBeenCalledTimes(1);
    expect(spellingEntries(landed).map((entry) => entry.word)).toEqual(parts.map(() => "word"));
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });
});

describe("checkRanges", () => {
  function checkedState(doc: string, extensions: Extension[] = [spellingPending]): EditorState {
    const state = EditorState.create({ doc, extensions });
    return state.update({ effects: setSpellingLintsIn.of([]) }).state;
  }

  function sent(state: EditorState): string[] {
    return checkRanges(state).map((range) => state.doc.sliceString(range.from, range.to));
  }

  function insertAt(state: EditorState, at: number, text: string): EditorState {
    return state.update({ changes: { from: at, insert: text } }).state;
  }

  it("is the whole document before the first check", () => {
    const state = EditorState.create({ doc: "one\n\ntwo", extensions: [spellingPending] });
    expect(sent(state)).toEqual(["one\n\ntwo"]);
  });

  it("is nothing once a result lands and nothing changed after it", () => {
    expect(checkRanges(checkedState("one\n\ntwo"))).toEqual([]);
  });

  it("is the paragraph around an edit", () => {
    const doc = "first line\nsecond line\n\nthird line\nfourth line";
    const state = insertAt(checkedState(doc), doc.indexOf("fourth"), "the ");
    expect(sent(state)).toEqual(["third line\nthe fourth line"]);
  });

  it("keeps an earlier edit's paragraph through the edits after it", () => {
    const doc = "alpha\n\nbeta\n\ngamma";
    let state = insertAt(checkedState(doc), doc.indexOf("gamma"), "x");
    state = insertAt(state, 0, "new line\n\n");
    expect(sent(state)).toEqual(["new line\n\nalpha", "xgamma"]);
  });

  it("reaches no more than 32 lines each way through a document with no blank lines", () => {
    const lines = Array.from({ length: 200 }, (_, i) => `line ${i}`);
    const doc = lines.join("\n");
    const state = insertAt(checkedState(doc), doc.indexOf("line 100"), "x");
    const [range] = sent(state);
    expect(range.split("\n")).toEqual(["line 68", ...lines.slice(69, 100), "xline 100", ...lines.slice(101, 133)]);
  });

  it("takes in the whole fenced block an edit lands in", () => {
    const doc = "Prose before.\n\n```\nfirst code line\n\nsecond code line\n```\n\nProse after.";
    const state = insertAt(checkedState(doc, [spellingPending, markdown()]), doc.indexOf("second code"), "x");
    expect(sent(state)).toEqual(["```\nfirst code line\n\nxsecond code line\n```"]);
  });
  it("is the whole document again once a recheck is asked for", () => {
    const doc = "one\n\ntwo";
    const state = checkedState(doc).update({ effects: recheckSpelling.of() }).state;
    expect(sent(state)).toEqual([doc]);
  });

  it("keeps a recheck through the edits after it until a result lands", () => {
    const doc = "one\n\ntwo";
    let state = checkedState(doc).update({ effects: recheckSpelling.of() }).state;
    state = insertAt(state, doc.length, " three");
    expect(sent(state)).toEqual(["one\n\ntwo three"]);
    state = state.update({ effects: setSpellingLintsIn.of([]) }).state;
    expect(checkRanges(state)).toEqual([]);
  });

  it("holds one stretch from the first edit to the last once a transaction changes more stretches than a check sends", () => {
    const parts = paragraphs(200);
    const doc = parts.join("\n\n");
    const changes = replaceAll(doc, "teh", "the", doc.indexOf("paragraph 50 "), doc.indexOf("paragraph 150 "));
    expect(changes).toHaveLength(100);
    const state = checkedState(doc).update({ changes }).state;

    expect(state.field(spellingPending)).toEqual([{ from: changes[0].from, to: changes[99].to }]);
    expect(sent(state)).toEqual([parts.slice(50, 150).join("\n\n").replaceAll("teh", "the")]);
  });

  it("keeps each edit's own stretch up to the number a check sends", () => {
    const doc = paragraphs(MAX_CHECKED_STRETCHES + 1).join("\n\n");
    const changes = replaceAll(doc, "teh", "the");
    const upTo = checkedState(doc).update({ changes: changes.slice(0, MAX_CHECKED_STRETCHES) }).state;
    expect(upTo.field(spellingPending)).toHaveLength(MAX_CHECKED_STRETCHES);
    expect(checkRanges(upTo)).toHaveLength(MAX_CHECKED_STRETCHES);

    const past = checkedState(doc).update({ changes }).state;
    expect(past.field(spellingPending)).toEqual([
      { from: changes[0].from, to: changes[changes.length - 1].to },
    ]);
  });

  it("never holds more stretches than a check sends, however many edits come before it", () => {
    const parts = paragraphs(60);
    const doc = parts.join("\n\n");
    let state = checkedState(doc);
    for (let i = parts.length - 1; i >= 0; i--) {
      state = insertAt(state, doc.indexOf(`paragraph ${i} `), "x");
      expect(state.field(spellingPending).length).toBeLessThanOrEqual(MAX_CHECKED_STRETCHES);
    }
    expect(checkRanges(state).length).toBeLessThanOrEqual(MAX_CHECKED_STRETCHES);
    expect(sent(state).join("\n\n")).toBe(state.doc.toString());
  });

  it("sends a block two edits widen to once, beside the other edits' paragraphs", () => {
    const doc = "Prose one.\n\n```\nfirst code line\n\nsecond code line\n```\n\nProse two.";
    let state = checkedState(doc, [spellingPending, markdown()]);
    state = insertAt(state, doc.indexOf("Prose two"), "x");
    state = insertAt(state, doc.indexOf("second code"), "y");
    state = insertAt(state, doc.indexOf("first code"), "z");
    expect(sent(state)).toEqual(["```\nzfirst code line\n\nysecond code line\n```", "xProse two."]);
  });
});

describe("sameSpellingRules", () => {
  it("holds for the same dialect and ignore list, in different arrays", () => {
    expect(
      sameSpellingRules(
        { dialect: "british", ignored_words: ["teh", "writ"] },
        { dialect: "british", ignored_words: ["teh", "writ"] },
      ),
    ).toBe(true);
  });

  it("fails for another dialect", () => {
    expect(
      sameSpellingRules({ dialect: "american", ignored_words: [] }, { dialect: "british", ignored_words: [] }),
    ).toBe(false);
  });

  it("fails for another ignore list", () => {
    expect(
      sameSpellingRules({ dialect: "american", ignored_words: ["teh"] }, { dialect: "american", ignored_words: [] }),
    ).toBe(false);
    expect(
      sameSpellingRules({ dialect: "american", ignored_words: ["teh"] }, { dialect: "american", ignored_words: ["the"] }),
    ).toBe(false);
  });
});
