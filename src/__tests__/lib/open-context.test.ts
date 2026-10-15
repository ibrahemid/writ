import { describe, it, expect } from "vitest";
import { EditorSelection, EditorState } from "@codemirror/state";

import { MAX_SELECTED_UNITS, pointAt, windowContext } from "../../lib/open-context";

function editor(doc: string, ranges: Array<[number, number]>, main = 0): EditorState {
  return EditorState.create({
    doc,
    extensions: EditorState.allowMultipleSelections.of(true),
    selection: EditorSelection.create(
      ranges.map(([anchor, head]) => EditorSelection.range(anchor, head)),
      main,
    ),
  });
}

describe("the open file as a program allowed to see it is answered", () => {
  it("counts lines from one and columns in characters", () => {
    const state = editor("# Plan\nhé🙂 there\n", [[0, 0]]);
    expect(pointAt(state, 0)).toEqual({ line: 1, column: 1 });
    expect(pointAt(state, 7)).toEqual({ line: 2, column: 1 });
    expect(pointAt(state, 7 + "hé🙂".length)).toEqual({ line: 2, column: 4 });
  });

  it("sends each selection with where it starts and ends, and the caret", () => {
    const doc = "# Plan\nlaunch the site\nthen the app\n";
    const state = editor(doc, [[7, 22], [1, 1], [28, 31]], 0);

    const answer = windowContext("tab-1", state, true);

    expect(answer.buffer_id).toBe("tab-1");
    expect(answer.unsaved).toBe(true);
    expect(answer.truncated).toBe(false);
    expect(answer.selections).toEqual([
      { text: "launch the site", from: { line: 2, column: 1 }, to: { line: 2, column: 16 } },
      { text: "the", from: { line: 3, column: 6 }, to: { line: 3, column: 9 } },
    ]);
    expect(answer.cursor).toEqual({ line: 2, column: 16 });
  });

  it("answers no tab when none is in front", () => {
    expect(windowContext(null, editor("x", [[0, 1]]), true)).toEqual({
      buffer_id: null,
      selections: [],
      cursor: null,
      unsaved: false,
      truncated: false,
    });
    expect(windowContext("tab-1", null, true).buffer_id).toBeNull();
  });

  it("holds a select-all of a long file to its ceiling and says so", () => {
    const doc = "a".repeat(MAX_SELECTED_UNITS + 10);
    const answer = windowContext("tab-1", editor(doc, [[0, doc.length]]), false);
    expect(answer.selections[0].text.length).toBe(MAX_SELECTED_UNITS);
    expect(answer.selections[0].to).toEqual({ line: 1, column: MAX_SELECTED_UNITS + 1 });
    expect(answer.truncated).toBe(true);
  });

  it("never cuts a character in half", () => {
    const doc = "a".repeat(MAX_SELECTED_UNITS - 1) + "🙂 tail";
    const answer = windowContext("tab-1", editor(doc, [[0, doc.length]]), false);
    expect(answer.selections[0].text.length).toBe(MAX_SELECTED_UNITS - 1);
    expect(answer.truncated).toBe(true);
  });
});
