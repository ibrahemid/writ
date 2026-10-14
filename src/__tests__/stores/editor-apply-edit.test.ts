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

  // F20: the range was read before the IPC and dispatched unmapped, so typing
  // above it during the round trip was overwritten ("X alpha beta" became
  // "ALPHA BETAta").
  describe("while the transform runs", () => {
    function deferred() {
      let resolve!: (value: string) => void;
      const promise = new Promise<string>((r) => (resolve = r));
      return { promise, resolve };
    }

    it("keeps typing above the range and lands the result on the text it was computed from", async () => {
      const view = makeView("first\nalpha beta", { from: 6, to: 16 });
      editorStore.registerView(view);
      const pending = deferred();

      const running = editorStore.applyEditToActiveBuffer({
        useSelectionIfPresent: true,
        transform: () => pending.promise,
      });
      view.dispatch({ changes: { from: 0, insert: "X " } });
      pending.resolve("ALPHA BETA");
      const result = await running;

      expect(view.state.doc.toString()).toBe("X first\nALPHA BETA");
      expect(result).toEqual({ applied: true, usedSelection: true, outputLength: 10 });
      expect(view.state.selection.main.from).toBe(8);
      expect(view.state.selection.main.to).toBe(18);
      view.destroy();
    });

    it("keeps typing below the range", async () => {
      const view = makeView("alpha beta\nlast", { from: 0, to: 10 });
      editorStore.registerView(view);
      const pending = deferred();

      const running = editorStore.applyEditToActiveBuffer({
        useSelectionIfPresent: true,
        transform: () => pending.promise,
      });
      view.dispatch({ changes: { from: view.state.doc.length, insert: " line" } });
      pending.resolve("ALPHA BETA");
      await running;

      expect(view.state.doc.toString()).toBe("ALPHA BETA\nlast line");
      view.destroy();
    });

    it("replaces nothing when an edit lands inside the range", async () => {
      const view = makeView("alpha beta", { from: 0, to: 10 });
      editorStore.registerView(view);
      const pending = deferred();

      const running = editorStore.applyEditToActiveBuffer({
        useSelectionIfPresent: true,
        transform: () => pending.promise,
      });
      view.dispatch({ changes: { from: 5, insert: "!" } });
      pending.resolve("ALPHA BETA");

      expect(await running).toEqual({ applied: false, reason: "document-changed" });
      expect(view.state.doc.toString()).toBe("alpha! beta");
      view.destroy();
    });

    it("replaces nothing on a whole-file transform once anything is typed", async () => {
      const view = makeView("alpha beta");
      editorStore.registerView(view);
      const pending = deferred();

      const running = editorStore.applyEditToActiveBuffer({
        useSelectionIfPresent: true,
        transform: () => pending.promise,
      });
      view.dispatch({ changes: { from: 0, insert: "X" } });
      pending.resolve("ALPHA BETA");

      expect(await running).toEqual({ applied: false, reason: "document-changed" });
      expect(view.state.doc.toString()).toBe("Xalpha beta");
      view.destroy();
    });

    it("replaces nothing when the view's state was swapped for another file", async () => {
      const view = makeView("alpha beta", { from: 0, to: 5 });
      editorStore.registerView(view);
      const pending = deferred();

      const running = editorStore.applyEditToActiveBuffer({
        useSelectionIfPresent: true,
        transform: () => pending.promise,
      });
      view.setState(EditorState.create({ doc: "another file" }));
      pending.resolve("ALPHA");

      expect(await running).toEqual({ applied: false, reason: "document-changed" });
      expect(view.state.doc.toString()).toBe("another file");
      view.destroy();
    });

    it("replaces nothing in a view that was destroyed", async () => {
      const view = makeView("alpha beta", { from: 0, to: 5 });
      editorStore.registerView(view);
      const pending = deferred();

      const running = editorStore.applyEditToActiveBuffer({
        useSelectionIfPresent: true,
        transform: () => pending.promise,
      });
      view.destroy();
      pending.resolve("ALPHA");

      expect(await running).toEqual({ applied: false, reason: "document-changed" });
    });
  });

  it("does not run the transform on a read-only file", async () => {
    const state = EditorState.create({
      doc: "hello",
      extensions: [EditorState.readOnly.of(true)],
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const view = new EditorView({ state, parent: container });
    editorStore.registerView(view);
    const transform = vi.fn(async (s: string) => s.toUpperCase());

    const result = await editorStore.applyEditToActiveBuffer({
      useSelectionIfPresent: true,
      transform,
    });

    expect(result).toEqual({ applied: false, reason: "read-only" });
    expect(transform).not.toHaveBeenCalled();
    expect(view.state.doc.toString()).toBe("hello");
    view.destroy();
  });
});
