import { describe, it, expect } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { autoTextDirection, buildAutoDirectionDecorations } from "../../editor/bidi";

const MIXED_DOC = ["مرحبا بالعالم", "const greeting = 1;", "سطر عربي آخر"].join("\n");

function makeView(doc: string, extensions = [autoTextDirection]) {
  const state = EditorState.create({ doc, extensions });
  const container = document.createElement("div");
  document.body.appendChild(container);
  return new EditorView({ state, parent: container });
}

function decorationStarts(view: EditorView): number[] {
  const starts: number[] = [];
  buildAutoDirectionDecorations(view).between(0, view.state.doc.length, (from) => {
    starts.push(from);
  });
  return starts;
}

describe("autoTextDirection", () => {
  it("enables per-line text direction reading", () => {
    const state = EditorState.create({ doc: MIXED_DOC, extensions: [autoTextDirection] });
    expect(state.facet(EditorView.perLineTextDirection)).toBe(true);
  });

  it("leaves the facet off when the extension is absent", () => {
    const state = EditorState.create({ doc: MIXED_DOC });
    expect(state.facet(EditorView.perLineTextDirection)).toBe(false);
  });

  it("marks every rendered line with dir=auto", () => {
    const view = makeView(MIXED_DOC);
    const lines = Array.from(view.contentDOM.querySelectorAll(".cm-line"));
    expect(lines.length).toBe(3);
    for (const line of lines) {
      expect(line.getAttribute("dir")).toBe("auto");
    }
    view.destroy();
  });

  it("does not touch the gutter, so line numbers stay left", () => {
    const view = makeView(MIXED_DOC);
    const gutters = view.dom.querySelectorAll(".cm-gutters [dir]");
    expect(gutters.length).toBe(0);
    view.destroy();
  });

  it("renders no dir attribute without the extension", () => {
    const view = makeView(MIXED_DOC, []);
    const dirty = view.contentDOM.querySelectorAll(".cm-line[dir]");
    expect(dirty.length).toBe(0);
    view.destroy();
  });

  it("emits one decoration per line, anchored at line starts", () => {
    const view = makeView(MIXED_DOC);
    const { doc } = view.state;
    expect(decorationStarts(view)).toEqual([
      doc.line(1).from,
      doc.line(2).from,
      doc.line(3).from,
    ]);
    view.destroy();
  });

  it("tracks lines added after the initial render", () => {
    const view = makeView(MIXED_DOC);
    view.dispatch({
      changes: { from: view.state.doc.length, insert: "\nنص جديد" },
    });
    expect(decorationStarts(view)).toHaveLength(4);
    const lines = Array.from(view.contentDOM.querySelectorAll(".cm-line"));
    expect(lines).toHaveLength(4);
    for (const line of lines) {
      expect(line.getAttribute("dir")).toBe("auto");
    }
    view.destroy();
  });
});
