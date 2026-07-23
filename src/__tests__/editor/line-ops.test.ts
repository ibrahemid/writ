import { describe, it, expect } from "vitest";
import { EditorSelection, EditorState, type SelectionRange } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  duplicateSelectionOrLine,
  insertBlankLineAbove,
  joinLines,
} from "../../editor/line-ops";

function makeView(
  doc: string,
  ranges?: SelectionRange | SelectionRange[],
  readOnly = false,
): EditorView {
  const selection = ranges
    ? EditorSelection.create(Array.isArray(ranges) ? ranges : [ranges])
    : undefined;
  const state = EditorState.create({
    doc,
    selection,
    extensions: [
      EditorState.allowMultipleSelections.of(true),
      ...(readOnly ? [EditorState.readOnly.of(true)] : []),
    ],
  });
  return new EditorView({ state });
}

describe("duplicateSelectionOrLine", () => {
  it("copies the line below a bare cursor and keeps the column", () => {
    const view = makeView("abc\ndef", EditorSelection.cursor(1));
    expect(duplicateSelectionOrLine(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("abc\nabc\ndef");
    // cursor lands on the copied line at column 1
    expect(view.state.selection.main.head).toBe(5);
    view.destroy();
  });

  it("copies a non-empty selection and selects the copy", () => {
    const view = makeView("hello", EditorSelection.range(0, 5));
    expect(duplicateSelectionOrLine(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("hellohello");
    const main = view.state.selection.main;
    expect([main.from, main.to]).toEqual([5, 10]);
    view.destroy();
  });

  it("duplicates the last line with a bare cursor", () => {
    const view = makeView("abc", EditorSelection.cursor(1));
    expect(duplicateSelectionOrLine(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("abc\nabc");
    view.destroy();
  });

  it("is multi-cursor safe: each cursor duplicates its own line in one step", () => {
    const view = makeView(
      "one\ntwo\nthree",
      [EditorSelection.cursor(0), EditorSelection.cursor(8)],
    );
    expect(duplicateSelectionOrLine(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("one\none\ntwo\nthree\nthree");
    view.destroy();
  });

  it("returns false on a read-only view and leaves the document unchanged", () => {
    const view = makeView("abc", EditorSelection.cursor(1), true);
    expect(duplicateSelectionOrLine(view)).toBe(false);
    expect(view.state.doc.toString()).toBe("abc");
    view.destroy();
  });
});

describe("joinLines", () => {
  it("joins the cursor line with the next, collapsing the boundary and indentation", () => {
    const view = makeView("foo  \n  bar\nbaz", EditorSelection.cursor(0));
    expect(joinLines(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("foo bar\nbaz");
    view.destroy();
  });

  it("returns false for a bare cursor on the last line", () => {
    const view = makeView("only", EditorSelection.cursor(2));
    expect(joinLines(view)).toBe(false);
    expect(view.state.doc.toString()).toBe("only");
    view.destroy();
  });

  it("joins every line a selection spans", () => {
    const view = makeView("a\nb\nc\nd", EditorSelection.range(0, 5));
    expect(joinLines(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("a b c\nd");
    view.destroy();
  });

  it("collapses trailing whitespace on the first line to a single space", () => {
    const view = makeView("a   \n\t b", EditorSelection.cursor(0));
    expect(joinLines(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("a b");
    view.destroy();
  });
});

describe("insertBlankLineAbove", () => {
  it("inserts a blank line above carrying the reference line's indentation", () => {
    const view = makeView("    hello", EditorSelection.cursor(6));
    expect(insertBlankLineAbove(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("    \n    hello");
    // cursor sits at the end of the new line's indentation
    expect(view.state.selection.main.head).toBe(4);
    view.destroy();
  });

  it("works on the first line", () => {
    const view = makeView("first\nsecond", EditorSelection.cursor(2));
    expect(insertBlankLineAbove(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("\nfirst\nsecond");
    expect(view.state.selection.main.head).toBe(0);
    view.destroy();
  });
});
