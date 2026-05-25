import { describe, it, expect, vi, beforeEach } from "vitest";
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

describe("editorStore.applyEditToActiveBuffer", () => {
  let editorStore: EditorStore;

  beforeEach(() => {
    editorStore = createEditorStore();
  });

  it("returns no-active-view when nothing is registered", async () => {
    const result = await editorStore.applyEditToActiveBuffer({
      useSelectionIfPresent: true,
      transform: async (s) => s.toUpperCase(),
    });
    expect(result).toEqual({ applied: false, reason: "no-active-view" });
  });

  it("transforms the entire document when no selection is present", async () => {
    const view = makeView("hello world");
    editorStore.registerView(view);

    const transform = vi.fn(async (s: string) => s.toUpperCase());
    const result = await editorStore.applyEditToActiveBuffer({
      useSelectionIfPresent: true,
      transform,
    });

    expect(transform).toHaveBeenCalledWith("hello world");
    expect(view.state.doc.toString()).toBe("HELLO WORLD");
    expect(result).toEqual({ applied: true, usedSelection: false, outputLength: 11 });
    view.destroy();
  });

  it("transforms only the selection when one is present", async () => {
    const view = makeView("hello world", { from: 0, to: 5 });
    editorStore.registerView(view);

    const transform = vi.fn(async (s: string) => s.toUpperCase());
    const result = await editorStore.applyEditToActiveBuffer({
      useSelectionIfPresent: true,
      transform,
    });

    expect(transform).toHaveBeenCalledWith("hello");
    expect(view.state.doc.toString()).toBe("HELLO world");
    expect(result).toEqual({ applied: true, usedSelection: true, outputLength: 5 });
    view.destroy();
  });

  it("collapses selection to the end of the inserted text", async () => {
    const view = makeView("aaa bbb ccc", { from: 4, to: 7 });
    editorStore.registerView(view);

    await editorStore.applyEditToActiveBuffer({
      useSelectionIfPresent: true,
      transform: async () => "BB",
    });

    const main = view.state.selection.main;
    expect(view.state.doc.toString()).toBe("aaa BB ccc");
    expect(main.from).toBe(4);
    expect(main.to).toBe(6);
    expect(main.empty).toBe(false);
    view.destroy();
  });

  it("ignores selection when useSelectionIfPresent is false", async () => {
    const view = makeView("hello world", { from: 0, to: 5 });
    editorStore.registerView(view);

    const transform = vi.fn(async (s: string) => `[${s}]`);
    const result = await editorStore.applyEditToActiveBuffer({
      useSelectionIfPresent: false,
      transform,
    });

    expect(transform).toHaveBeenCalledWith("hello world");
    expect(view.state.doc.toString()).toBe("[hello world]");
    expect(result).toEqual({ applied: true, usedSelection: false, outputLength: 13 });
    view.destroy();
  });

  it("returns transform-error when the transform throws", async () => {
    const view = makeView("hello");
    editorStore.registerView(view);

    const boom = new Error("boom");
    const result = await editorStore.applyEditToActiveBuffer({
      useSelectionIfPresent: true,
      transform: async () => {
        throw boom;
      },
    });

    expect(result).toEqual({ applied: false, reason: "transform-error", error: boom });
    expect(view.state.doc.toString()).toBe("hello");
    view.destroy();
  });
});
