import { describe, it, expect, vi, beforeEach } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState, EditorSelection } from "@codemirror/state";

const mocks = vi.hoisted(() => ({
  readClipboardText: vi.fn(),
  writeClipboardText: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  readClipboardText: mocks.readClipboardText,
  writeClipboardText: mocks.writeClipboardText,
}));

import {
  copySelection,
  cutSelection,
  pasteIntoSelection,
} from "../../editor/clipboard-commands";
import type { ImagePasteDeps } from "../../editor/image-paste";
import type { ClipboardPasteResult } from "../../types/image-import";

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

// EP-22: the context menu copied and cut the main range only, while Cmd+C
// copies every range.
describe("with several selection ranges", () => {
  function multiView(doc: string, ranges: Array<[number, number]>) {
    const state = EditorState.create({
      doc,
      selection: EditorSelection.create(ranges.map(([from, to]) => EditorSelection.range(from, to))),
      extensions: [EditorState.allowMultipleSelections.of(true)],
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    return new EditorView({ state, parent: container });
  }

  it("copies every range, one per line", async () => {
    await copySelection(multiView("one two three", [[0, 3], [4, 7], [8, 13]]));
    expect(mocks.writeClipboardText).toHaveBeenCalledWith("one\ntwo\nthree");
  });

  it("skips empty ranges", async () => {
    await copySelection(multiView("one two", [[0, 3], [5, 5]]));
    expect(mocks.writeClipboardText).toHaveBeenCalledWith("one");
  });

  it("cuts every range and leaves a caret at each", async () => {
    const view = multiView("one two three", [[0, 3], [8, 13]]);
    await cutSelection(view);
    expect(mocks.writeClipboardText).toHaveBeenCalledWith("one\nthree");
    expect(view.state.doc.toString()).toBe(" two ");
    expect(view.state.selection.ranges.map((r) => [r.from, r.to])).toEqual([
      [0, 0],
      [5, 5],
    ]);
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

describe("paste into a Markdown file", () => {
  function imageDeps(acceptsImages: boolean, result: ClipboardPasteResult): ImagePasteDeps {
    return {
      bufferId: () => "buf-1",
      acceptsImages: () => acceptsImages,
      pasteImages: vi.fn(() => Promise.resolve(result)),
      readText: vi.fn(() => Promise.resolve("typed")),
      notify: vi.fn(),
      logFailure: vi.fn(),
    };
  }

  it("asks the host for images and inserts their references", async () => {
    const deps = imageDeps(true, { kind: "inserted", markdown: "![](assets/a.png)", notices: [] });
    const view = makeView("x", { from: 1, to: 1 });
    await pasteIntoSelection(view, deps);
    expect(deps.pasteImages).toHaveBeenCalledWith("buf-1");
    expect(view.state.doc.toString()).toBe("x![](assets/a.png)");
    expect(mocks.readClipboardText).not.toHaveBeenCalled();
  });

  it("pastes text through the same path when there are no images", async () => {
    const deps = imageDeps(true, { kind: "text" });
    const view = makeView("", { from: 0, to: 0 });
    await pasteIntoSelection(view, deps);
    expect(view.state.doc.toString()).toBe("typed");
  });

  it("pastes text as before in a tab that takes no images", async () => {
    mocks.readClipboardText.mockResolvedValue("plain");
    const deps = imageDeps(false, { kind: "text" });
    const view = makeView("", { from: 0, to: 0 });
    await pasteIntoSelection(view, deps);
    expect(deps.pasteImages).not.toHaveBeenCalled();
    expect(view.state.doc.toString()).toBe("plain");
  });
});
