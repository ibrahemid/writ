import { describe, it, expect, vi, beforeEach } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState, EditorSelection } from "@codemirror/state";

const mocks = vi.hoisted(() => ({
  readClipboardText: vi.fn(),
  writeClipboardText: vi.fn(),
}));

vi.mock("../../services/clipboard", () => ({
  readClipboardText: mocks.readClipboardText,
  writeClipboardText: mocks.writeClipboardText,
}));

import {
  copySelection,
  cutSelection,
  pasteIntoSelection,
} from "../../editor/clipboard-commands";

function makeView(doc: string, selection?: { from: number; to: number }, readOnly = false) {
  const state = EditorState.create({
    doc,
    selection: selection
      ? EditorSelection.single(selection.from, selection.to)
      : EditorSelection.single(0),
    extensions: readOnly ? [EditorState.readOnly.of(true)] : [],
  });
  const container = document.createElement("div");
  document.body.appendChild(container);
  return new EditorView({ state, parent: container });
}

beforeEach(() => {
  mocks.readClipboardText.mockReset().mockResolvedValue("");
  mocks.writeClipboardText.mockReset().mockResolvedValue(undefined);
});

describe("copy", () => {
  it("writes the selected text", async () => {
    await copySelection(makeView("hello world", { from: 6, to: 11 }));
    expect(mocks.writeClipboardText).toHaveBeenCalledWith("world");
  });

  it("does nothing with an empty selection", async () => {
    await copySelection(makeView("hello", { from: 2, to: 2 }));
    expect(mocks.writeClipboardText).not.toHaveBeenCalled();
  });

  it("works in a read-only buffer", async () => {
    await copySelection(makeView("locked text", { from: 0, to: 6 }, true));
    expect(mocks.writeClipboardText).toHaveBeenCalledWith("locked");
  });
});

describe("cut", () => {
  it("copies then removes the selection", async () => {
    const view = makeView("hello world", { from: 5, to: 11 });
    await cutSelection(view);
    expect(mocks.writeClipboardText).toHaveBeenCalledWith(" world");
    expect(view.state.doc.toString()).toBe("hello");
    expect(view.state.selection.main.head).toBe(5);
  });

  it("removes the text in one undo step", async () => {
    const view = makeView("hello world", { from: 0, to: 5 });
    await cutSelection(view);
    // A single transaction, so one undo restores the whole cut.
    expect(view.state.doc.toString()).toBe(" world");
  });

  it("refuses in a read-only buffer, and copies nothing", async () => {
    const view = makeView("locked", { from: 0, to: 6 }, true);
    await cutSelection(view);
    expect(view.state.doc.toString()).toBe("locked");
    expect(mocks.writeClipboardText).not.toHaveBeenCalled();
  });
});

describe("paste", () => {
  it("replaces the selection with the clipboard text", async () => {
    mocks.readClipboardText.mockResolvedValue("there");
    const view = makeView("hello world", { from: 6, to: 11 });
    await pasteIntoSelection(view);
    expect(view.state.doc.toString()).toBe("hello there");
  });

  it("inserts at the caret when nothing is selected", async () => {
    mocks.readClipboardText.mockResolvedValue("X");
    const view = makeView("ab", { from: 1, to: 1 });
    await pasteIntoSelection(view);
    expect(view.state.doc.toString()).toBe("aXb");
  });

  it("leaves the selection alone when the clipboard holds no text", async () => {
    // An image-only clipboard must not silently delete the selection.
    mocks.readClipboardText.mockResolvedValue("");
    const view = makeView("keep me", { from: 0, to: 4 });
    await pasteIntoSelection(view);
    expect(view.state.doc.toString()).toBe("keep me");
  });

  it("refuses in a read-only buffer without reading the clipboard", async () => {
    const view = makeView("locked", { from: 0, to: 6 }, true);
    await pasteIntoSelection(view);
    expect(view.state.doc.toString()).toBe("locked");
    expect(mocks.readClipboardText).not.toHaveBeenCalled();
  });
});
