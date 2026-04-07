import { describe, it, expect, afterEach } from "vitest";
import { EditorState, EditorSelection } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { addCursorUp, addCursorDown } from "../../commands/multicursor";

let activeView: EditorView | undefined;

function createView(doc: string, cursorPos?: number): EditorView {
  activeView?.destroy();
  const state = EditorState.create({
    doc,
    extensions: [EditorState.allowMultipleSelections.of(true)],
    selection: cursorPos !== undefined
      ? EditorSelection.single(cursorPos)
      : undefined,
  });
  activeView = new EditorView({ state });
  return activeView;
}

function createViewWithCursors(doc: string, positions: number[]): EditorView {
  activeView?.destroy();
  const ranges = positions.map((p) => EditorSelection.cursor(p));
  const state = EditorState.create({
    doc,
    extensions: [EditorState.allowMultipleSelections.of(true)],
    selection: EditorSelection.create(ranges),
  });
  activeView = new EditorView({ state });
  return activeView;
}

function cursorPositions(view: EditorView): number[] {
  return view.state.selection.ranges.map((r) => r.head).sort((a, b) => a - b);
}

afterEach(() => {
  activeView?.destroy();
  activeView = undefined;
});

describe("addCursorDown", () => {
  it("adds a cursor on the line below at the same column", () => {
    const view = createView("hello\nworld", 2);

    const handled = addCursorDown(view);

    expect(handled).toBe(true);
    expect(cursorPositions(view)).toEqual([2, 8]);
  });

  it("returns false when cursor is on the last line", () => {
    const view = createView("hello\nworld", 8);

    const handled = addCursorDown(view);

    expect(handled).toBe(false);
    expect(cursorPositions(view)).toEqual([8]);
  });

  it("clamps to end of shorter line below", () => {
    const view = createView("long line\nhi", 7);

    addCursorDown(view);

    expect(cursorPositions(view)).toEqual([7, 12]);
  });

  it("adds cursors from multiple existing cursors and deduplicates overlapping positions", () => {
    const view = createViewWithCursors("aaa\nbbb\nccc", [1, 5]);

    addCursorDown(view);

    expect(cursorPositions(view)).toEqual([1, 5, 9]);
  });

  it("works with empty lines", () => {
    const view = createView("hello\n\nworld", 3);

    addCursorDown(view);

    expect(cursorPositions(view)).toEqual([3, 6]);
  });

  it("handles cursor at column 0", () => {
    const view = createView("abc\ndef", 0);

    addCursorDown(view);

    expect(cursorPositions(view)).toEqual([0, 4]);
  });

  it("handles single character lines", () => {
    const view = createView("a\nb\nc", 0);

    addCursorDown(view);

    expect(cursorPositions(view)).toEqual([0, 2]);
  });
});

describe("addCursorUp", () => {
  it("adds a cursor on the line above at the same column", () => {
    const view = createView("hello\nworld", 8);

    const handled = addCursorUp(view);

    expect(handled).toBe(true);
    expect(cursorPositions(view)).toEqual([2, 8]);
  });

  it("returns false when cursor is on the first line", () => {
    const view = createView("hello\nworld", 2);

    const handled = addCursorUp(view);

    expect(handled).toBe(false);
    expect(cursorPositions(view)).toEqual([2]);
  });

  it("clamps to end of shorter line above", () => {
    const view = createView("hi\nlong line", 10);

    addCursorUp(view);

    expect(cursorPositions(view)).toEqual([2, 10]);
  });

  it("works across multiple lines successively", () => {
    const view = createView("aaa\nbbb\nccc", 9);

    addCursorUp(view);
    addCursorUp(view);

    expect(cursorPositions(view)).toEqual([1, 5, 9]);
  });

  it("handles empty line above", () => {
    const view = createView("hello\n\nworld", 10);

    addCursorUp(view);

    expect(cursorPositions(view)).toEqual([6, 10]);
  });

  it("handles cursor at end of line clamped to shorter line above", () => {
    const view = createView("ab\nabcdef", 8);

    addCursorUp(view);

    expect(cursorPositions(view)).toEqual([2, 8]);
  });
});

describe("addCursorUp and addCursorDown combined", () => {
  it("creates cursors in both directions from middle line", () => {
    const view = createView("aaa\nbbb\nccc", 5);

    addCursorUp(view);
    addCursorDown(view);

    const positions = cursorPositions(view);
    expect(positions).toContain(1);
    expect(positions).toContain(5);
    expect(positions).toContain(9);
  });

  it("deduplicates overlapping cursor positions", () => {
    const view = createView("aa\nbb\ncc", 4);

    addCursorUp(view);
    addCursorDown(view);

    const positions = cursorPositions(view);
    const unique = [...new Set(positions)];
    expect(positions.length).toBe(unique.length);
  });

  it("handles large document with many lines", () => {
    const lines = Array.from({ length: 100 }, (_, i) => `line ${i.toString().padStart(3, "0")}`);
    const doc = lines.join("\n");
    const view = createView(doc, 53);

    for (let i = 0; i < 10; i++) {
      addCursorDown(view);
    }

    expect(view.state.selection.ranges.length).toBe(11);
  });
});
