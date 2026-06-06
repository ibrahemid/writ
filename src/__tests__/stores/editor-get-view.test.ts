import { describe, it, expect } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { createEditorStore } from "../../stores/window/editor-store";

function makeView(doc: string) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  return new EditorView({ state: EditorState.create({ doc }), parent: container });
}

describe("editorStore.getView", () => {
  it("returns null before any view is registered", () => {
    expect(createEditorStore().getView()).toBeNull();
  });

  it("returns the registered view and null after deregistration", () => {
    const store = createEditorStore();
    const view = makeView("hello");
    store.registerView(view);
    expect(store.getView()).toBe(view);
    store.registerView(null);
    expect(store.getView()).toBeNull();
    view.destroy();
  });
});
