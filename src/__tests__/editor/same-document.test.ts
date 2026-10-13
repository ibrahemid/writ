import { describe, it, expect, vi, afterEach } from "vitest";
import { EditorState, Text } from "@codemirror/state";
import { history, isolateHistory, redo, undo } from "@codemirror/commands";
import { sameDocument } from "../../editor/same-document";

function textOf(value: string): Text {
  return Text.of(value.split("\n"));
}

function seeded(seed: number): (below: number) => number {
  let state = seed;
  return (below) => {
    state = (state * 48271) % 2147483647;
    return Math.floor((state / 2147483647) * below);
  };
}

describe("sameDocument", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("answers as Text.eq does for small documents", () => {
    const pairs: Array<[string, string]> = [
      ["", ""],
      ["", "a"],
      ["a", "a"],
      ["a\nb", "ab\n"],
      ["one\ntwo", "one\ntwo"],
      ["one\ntwo", "one\ntwO"],
    ];
    for (const [a, b] of pairs) {
      expect(sameDocument(textOf(a), textOf(b))).toBe(a === b);
      expect(sameDocument(textOf(b), textOf(a))).toBe(a === b);
    }
  });

  it("answers as Text.eq does for documents built apart, which share nothing", () => {
    const value = Array.from({ length: 50_000 }, (_, i) => `line ${i}`).join("\n");
    expect(sameDocument(textOf(value), textOf(value))).toBe(true);
    const middle = value.indexOf("line 25000");
    const changed = `${value.slice(0, middle)}LINE${value.slice(middle + 4)}`;
    expect(sameDocument(textOf(value), textOf(changed))).toBe(false);
  });

  it("answers as Text.eq does through random edits, undo and redo of a large document", () => {
    const pick = seeded(20261013);
    const initial = Array.from({ length: 60_000 }, (_, i) => `line ${i} ${"x".repeat(pick(40))}`).join("\n");
    let state = EditorState.create({ doc: initial, extensions: [history()] });
    const apply = (tr: { state: EditorState }) => {
      state = tr.state;
    };
    const kept: Text[] = [state.doc];
    let equalSeen = 0;
    const check = () => {
      for (const text of kept) {
        const expected = text.eq(state.doc);
        expect(sameDocument(state.doc, text)).toBe(expected);
        expect(sameDocument(text, state.doc)).toBe(expected);
        if (expected) equalSeen++;
      }
    };

    for (let round = 0; round < 40; round++) {
      const before = state.doc;
      kept.push(before);
      const edits = 1 + pick(5);
      for (let i = 0; i < edits; i++) {
        const length = state.doc.length;
        const from = pick(length + 1);
        const reach = pick(10) === 0 ? 300_000 : 60;
        const to = Math.min(length, from + pick(reach));
        const insert =
          pick(5) === 0
            ? Array.from({ length: pick(3_000) }, (_, n) => `new ${n}`).join("\n")
            : ["", "a", "\n", "bc\nd"][pick(4)];
        apply(state.update({ changes: { from, to, insert }, annotations: isolateHistory.of("full") }));
        check();
      }
      for (let i = 0; i < edits; i++) {
        undo({ state, dispatch: apply });
        check();
      }
      expect(state.doc.eq(before)).toBe(true);
      expect(sameDocument(state.doc, before)).toBe(true);
      const again = pick(edits + 1);
      for (let i = 0; i < again; i++) {
        redo({ state, dispatch: apply });
        check();
      }
      if (kept.length > 12) kept.splice(1, 1);
    }
    expect(equalSeen).toBeGreaterThan(40);
  });

  it("hands Text.eq only the stretches an edit and its undo touched, however far apart", () => {
    const lines = Array.from({ length: 100_000 }, (_, i) => `line ${i} of the saved text`);
    let state = EditorState.create({ doc: lines.join("\n"), extensions: [history()] });
    const saved = state.doc;
    const apply = (tr: { state: EditorState }) => {
      state = tr.state;
    };
    apply(state.update({ changes: { from: 0, insert: "top " }, annotations: isolateHistory.of("full") }));
    apply(
      state.update({ changes: { from: state.doc.length, insert: " bottom" }, annotations: isolateHistory.of("full") }),
    );
    undo({ state, dispatch: apply });
    undo({ state, dispatch: apply });
    expect(state.doc).not.toBe(saved);

    const eq = vi.spyOn(Text.prototype, "eq");
    expect(sameDocument(state.doc, saved)).toBe(true);
    const read = eq.mock.contexts.reduce<number>((sum, text) => sum + (text as Text).length, 0);
    expect(read).toBeLessThan(16 * 1024);
  });
});
