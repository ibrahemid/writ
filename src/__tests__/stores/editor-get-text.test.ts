import { describe, it, expect, beforeEach } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState, EditorSelection } from "@codemirror/state";
import { createEditorStore, type EditorStore } from "../../stores/window/editor-store";

function makeView(initial: string, selection?: { from: number; to: number }) {
  const state = EditorState.create({
    doc: initial,
    selection: selection
      ? EditorSelection.single(selection.from, selection.to)
      : EditorSelection.single(0),
  });
  const container = document.createElement("div");
  document.body.appendChild(container);
  return new EditorView({ state, parent: container });
}

describe("editorStore.getActiveText", () => {
  let editorStore: EditorStore;

  beforeEach(() => {
    editorStore = createEditorStore();
  });

  it("returns null when nothing is registered", () => {
    expect(editorStore.getActiveText(true)).toBeNull();
  });

  it("returns the entire document when no selection is present", () => {
    const view = makeView("hello world");
    editorStore.registerView(view);

    expect(editorStore.getActiveText(true)).toEqual({
      text: "hello world",
      usedSelection: false,
    });
    expect(view.state.doc.toString()).toBe("hello world");
    view.destroy();
  });

  it("returns only the selection when one is present", () => {
    const view = makeView("hello world", { from: 0, to: 5 });
    editorStore.registerView(view);

    expect(editorStore.getActiveText(true)).toEqual({
      text: "hello",
      usedSelection: true,
    });
    view.destroy();
  });

  it("ignores the selection when useSelectionIfPresent is false", () => {
    const view = makeView("hello world", { from: 0, to: 5 });
    editorStore.registerView(view);

    expect(editorStore.getActiveText(false)).toEqual({
      text: "hello world",
      usedSelection: false,
    });
    view.destroy();
  });
});
