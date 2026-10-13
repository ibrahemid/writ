import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { indentUnit } from "@codemirror/language";
import {
  TAB_SIZE_MAX,
  TAB_SIZE_MIN,
  clampTabSize,
  indentationFor,
  lineWrappingFor,
} from "../../editor/editor-settings";
import type { FileOpenMode } from "../../types/buffer";

function wrapsIn(mode: FileOpenMode, wordWrap: boolean): boolean {
  const view = new EditorView({
    state: EditorState.create({ doc: "x", extensions: [lineWrappingFor(wordWrap, mode)] }),
  });
  const wraps = view.contentDOM.classList.contains("cm-lineWrapping");
  view.destroy();
  return wraps;
}

describe("lineWrappingFor", () => {
  it("wraps a normal file when Word wrap is on, and only then", () => {
    expect(wrapsIn({ kind: "Normal" }, true)).toBe(true);
    expect(wrapsIn({ kind: "Normal" }, false)).toBe(false);
  });

  it.each<FileOpenMode>([
    { kind: "LargeFile" },
    { kind: "LargeFileConfirm" },
    { kind: "LongLines" },
    { kind: "Binary" },
  ])("never wraps a $kind file", (mode) => {
    expect(wrapsIn(mode, true)).toBe(false);
  });
});

describe("indentationFor", () => {
  it.each([1, 2, 4, 8, 16])("draws a tab %i columns wide and indents by as many spaces", (size) => {
    const state = EditorState.create({ extensions: [indentationFor(size)] });
    expect(state.tabSize).toBe(size);
    expect(state.facet(indentUnit)).toBe(" ".repeat(size));
  });

  it("holds a hand-written value to the range Settings offers", () => {
    expect(EditorState.create({ extensions: [indentationFor(0)] }).tabSize).toBe(TAB_SIZE_MIN);
    expect(EditorState.create({ extensions: [indentationFor(64)] }).tabSize).toBe(TAB_SIZE_MAX);
  });
});

describe("clampTabSize", () => {
  it("keeps a value in range and rounds a fraction", () => {
    expect(clampTabSize(4)).toBe(4);
    expect(clampTabSize(3.6)).toBe(4);
  });

  it("pulls an out-of-range or non-numeric value into range", () => {
    expect(clampTabSize(-3)).toBe(TAB_SIZE_MIN);
    expect(clampTabSize(99)).toBe(TAB_SIZE_MAX);
    expect(clampTabSize(Number.NaN)).toBe(TAB_SIZE_MIN);
  });
});
