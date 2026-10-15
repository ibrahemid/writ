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
  readingMeasure,
  readingWidthFor,
} from "../../editor/editor-settings";
import { featuresFor } from "../../editor/large-file";
import type { FileOpenMode } from "../../types/buffer";

function wrapsIn(mode: FileOpenMode, wordWrap: boolean, longest = 0): boolean {
  const allowed = featuresFor(mode, longest).wrap;
  const view = new EditorView({
    state: EditorState.create({ doc: "x", extensions: [lineWrappingFor(wordWrap, allowed)] }),
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
    { kind: "Binary" },
  ])("does not wrap a $kind file", (mode) => {
    expect(wrapsIn(mode, true)).toBe(false);
  });

  it("wraps a long-lined file while its longest line is within the wrap limit", () => {
    expect(wrapsIn({ kind: "LongLines" }, true, 50_000)).toBe(true);
    expect(wrapsIn({ kind: "LongLines" }, true, 5_000_000)).toBe(false);
    expect(wrapsIn({ kind: "LongLines" }, false, 50_000)).toBe(false);
  });

  it("wraps a file the person turned wrapping on for, whatever its tier", () => {
    const allowed = featuresFor({ kind: "LargeFile" }, 0, { wrap: true }).wrap;
    const view = new EditorView({
      state: EditorState.create({ doc: "x", extensions: [lineWrappingFor(true, allowed)] }),
    });
    expect(view.contentDOM.classList.contains("cm-lineWrapping")).toBe(true);
    view.destroy();
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

describe("readingWidthFor", () => {
  function measureOf(enabled: boolean, code: boolean): string {
    const view = new EditorView({
      state: EditorState.create({ doc: "x", extensions: [readingWidthFor(enabled, code)] }),
    });
    const width = getComputedStyle(view.contentDOM).maxWidth;
    view.destroy();
    return width;
  }

  it("holds prose to the reading measure when the setting is on", () => {
    expect(readingWidthFor(true, false)).toBe(readingMeasure);
    expect(measureOf(true, false)).toBe("var(--writ-prose-measure)");
  });

  it("leaves prose at the pane's width when the setting is off", () => {
    expect(readingWidthFor(false, false)).toEqual([]);
    expect(measureOf(false, false)).not.toBe("var(--writ-prose-measure)");
  });

  it("leaves a source file at the pane's width either way", () => {
    expect(readingWidthFor(true, true)).toEqual([]);
  });
});
