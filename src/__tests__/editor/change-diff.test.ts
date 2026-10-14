import { describe, it, expect } from "vitest";
import { ChangeSet, EditorState, Text } from "@codemirror/state";
import { changesBetween, encodeChangesSince } from "../../editor/change-diff";

function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PIECES = ["a", "word ", "\n", "😀", "中文", "é", "\n\n", "line of text\n", "👍🏽", "  "];

function randomText(random: () => number, length: number): string {
  let out = "";
  while (out.length < length) out += PIECES[Math.floor(random() * PIECES.length)];
  return out;
}

/** A position in `doc` that does not split a surrogate pair. */
function boundary(doc: Text, random: () => number): number {
  let pos = Math.floor(random() * (doc.length + 1));
  if (pos > 0 && pos < doc.length) {
    const code = doc.sliceString(pos, pos + 1).charCodeAt(0);
    if (code >= 0xdc00 && code <= 0xdfff) pos--;
  }
  return pos;
}

function lines(count: number): Text {
  return Text.of(Array.from({ length: count }, (_, n) => `line ${n} of a long document`));
}

describe("changesBetween", () => {
  it("turns the document a write landed with into the one typed since, across random edits", () => {
    const random = seeded(32);
    for (let round = 0; round < 60; round++) {
      let state = EditorState.create({ doc: randomText(random, 2_000 + Math.floor(random() * 40_000)) });
      const base = state.doc;
      const edits = 1 + Math.floor(random() * 12);
      for (let n = 0; n < edits; n++) {
        const a = boundary(state.doc, random);
        const b = boundary(state.doc, random);
        const from = Math.min(a, b);
        const to = random() < 0.5 ? from : Math.max(a, b);
        const insert = random() < 0.2 ? "" : randomText(random, Math.floor(random() * 300));
        state = state.update({ changes: { from, to, insert } }).state;
      }
      const changes = changesBetween(base, state.doc);
      expect(changes.apply(base).eq(state.doc)).toBe(true);
      expect(ChangeSet.fromJSON(changes.toJSON()).apply(base).eq(state.doc)).toBe(true);
    }
  });

  it("sends a keystroke in a large document as a change the size of the keystroke", () => {
    const base = lines(200_000);
    const pos = base.line(150_000).from + 4;
    const doc = EditorState.create({ doc: base }).update({ changes: { from: pos, insert: "x" } }).state.doc;

    const json = JSON.stringify(changesBetween(base, doc).toJSON());

    expect(json).toBe(JSON.stringify([pos, [0, "x"], base.length - pos]));
  });

  it("sends edits near the top and the bottom without the text between them", () => {
    const base = lines(100_000);
    let state = EditorState.create({ doc: base });
    state = state.update({ changes: { from: 10, to: 11, insert: "TOP" } }).state;
    const end = state.doc.length - 3;
    state = state.update({ changes: { from: end, insert: "BOTTOM" } }).state;

    const changes = changesBetween(base, state.doc);

    expect(changes.apply(base).eq(state.doc)).toBe(true);
    expect(JSON.stringify(changes.toJSON()).length).toBeLessThan(200);
  });

  it("keeps an emoji whole when the edit sits beside it", () => {
    const base = Text.of(["a😀b"]);
    const doc = Text.of(["a😀c"]);
    const changes = changesBetween(base, doc);
    expect(changes.toJSON()).toEqual([3, [1, "c"]]);
    expect(changes.apply(base).toString()).toBe("a😀c");
  });

  it("answers no change for the same document", () => {
    const base = lines(10);
    expect(changesBetween(base, base).empty).toBe(true);
  });

  it("replaces everything between two documents that share nothing", () => {
    const base = lines(5_000);
    const doc = Text.of(["something else", "entirely"]);
    expect(changesBetween(base, doc).apply(base).eq(doc)).toBe(true);
  });

  it("covers deleting every line but the last and typing into an empty document", () => {
    const base = lines(3_000);
    const kept = Text.of(["line 2999 of a long document"]);
    const cut = EditorState.create({ doc: base }).update({ changes: { from: 0, to: base.line(3_000).from } }).state.doc;
    expect(cut.eq(kept)).toBe(true);
    expect(changesBetween(base, cut).apply(base).eq(cut)).toBe(true);

    const empty = Text.empty;
    const typed = EditorState.create({ doc: empty }).update({ changes: { from: 0, insert: "first" } }).state.doc;
    expect(changesBetween(empty, typed).toJSON()).toEqual([[0, "first"]]);
  });
});

describe("encodeChangesSince", () => {
  it("encodes the changes and the length of the document they produce", () => {
    const base = Text.of(["one", "two"]);
    const doc = Text.of(["one", "two", "three"]);
    expect(encodeChangesSince(base, doc)).toEqual({ changes: [7, [0, "", "three"]], length: 13 });
  });

  it("answers null for anything that is not a document", () => {
    expect(encodeChangesSince("text", Text.of(["a"]))).toBeNull();
    expect(encodeChangesSince(Text.of(["a"]), { length: 1, lines: 1, toString: () => "a" })).toBeNull();
  });
});
