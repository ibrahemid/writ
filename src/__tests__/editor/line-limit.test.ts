import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { rangesWithinLineLimit } from "../../editor/line-limit";
import { buildAutoDirectionDecorations } from "../../editor/bidi";

function viewOf(doc: string): EditorView {
  return new EditorView({ state: EditorState.create({ doc }) });
}

describe("rangesWithinLineLimit", () => {
  it("takes a line over the limit out of the visible ranges and keeps the lines around it", () => {
    const blob = "x".repeat(50);
    const view = viewOf(`short\n${blob}\nend`);
    expect(rangesWithinLineLimit(view, 10)).toEqual([
      { from: 0, to: 5 },
      { from: 57, to: 60 },
    ]);
    view.destroy();
  });

  it("leaves a document with no long line as the view shows it", () => {
    const view = viewOf("one\ntwo\nthree");
    expect(rangesWithinLineLimit(view, 10)).toEqual(view.visibleRanges.map(({ from, to }) => ({ from, to })));
    view.destroy();
  });

  it("answers nothing for a document that is one long line", () => {
    const view = viewOf("y".repeat(50));
    expect(rangesWithinLineLimit(view, 10)).toEqual([]);
    view.destroy();
  });
});

describe("per-line direction", () => {
  it("leaves a line over the limit in the editor's direction", () => {
    const view = viewOf(`مرحبا\n${"x".repeat(10_001)}\nhello`);
    const starts: number[] = [];
    buildAutoDirectionDecorations(view).between(0, view.state.doc.length, (from) => {
      starts.push(from);
    });
    expect(starts).toEqual([0, view.state.doc.line(3).from]);
    view.destroy();
  });
});
