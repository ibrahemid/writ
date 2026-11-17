import { describe, it, expect, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState, EditorSelection } from "@codemirror/state";
import { undo, history } from "@codemirror/commands";
import {
  CLIPBOARD_CHANGED_NOTICE,
  PASTE_FAILED_NOTICE,
  TAB_CHANGED_NOTICE,
  clipboardMayHoldImages,
  imagePaste,
  pasteImagesOrText,
  type ImagePasteDeps,
} from "../../editor/image-paste";
import type { ClipboardPasteResult } from "../../types/image-import";

function clipboard(types: string[], text = ""): DataTransfer {
  return {
    types,
    getData: (type: string) => (type === "text/plain" ? text : ""),
  } as unknown as DataTransfer;
}

function makeView(doc: string, at = doc.length, extensions: unknown[] = [], readOnly = false) {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.single(at),
    extensions: [
      history(),
      ...(readOnly ? [EditorState.readOnly.of(true)] : []),
      ...(extensions as never[]),
    ],
  });
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  return new EditorView({ state, parent });
}

function makeDeps(
  result: ClipboardPasteResult | Error,
  overrides: Partial<ImagePasteDeps> = {},
): ImagePasteDeps & { notices: string[]; logged: string[] } {
  const notices: string[] = [];
  const logged: string[] = [];
  return {
    notices,
    logged,
    bufferId: () => "buf-1",
    acceptsImages: () => true,
    pasteImages: vi.fn(() =>
      result instanceof Error ? Promise.reject(result) : Promise.resolve(result),
    ),
    readText: vi.fn(() => Promise.resolve("")),
    notify: (sent) => notices.push(...sent),
    logFailure: (message) => logged.push(message),
    ...overrides,
  };
}

describe("clipboardMayHoldImages", () => {
  it("is false without clipboard data", () => {
    expect(clipboardMayHoldImages(null)).toBe(false);
  });

  it("is true for copied files and for bitmaps", () => {
    expect(clipboardMayHoldImages(clipboard(["Files", "text/plain"], "shot.png"))).toBe(true);
    expect(clipboardMayHoldImages(clipboard(["image/png"]))).toBe(true);
  });

  it("is true when the clipboard holds no text the webview can read", () => {
    expect(clipboardMayHoldImages(clipboard([]))).toBe(true);
  });

  it("is true for text that is only file names", () => {
    expect(clipboardMayHoldImages(clipboard(["text/plain"], "shot.png"))).toBe(true);
    expect(clipboardMayHoldImages(clipboard(["text/plain"], "a b.PNG\r\nc.jpeg\n"))).toBe(true);
  });

  it("is false for ordinary text", () => {
    expect(clipboardMayHoldImages(clipboard(["text/plain"], "hello world"))).toBe(false);
    expect(clipboardMayHoldImages(clipboard(["text/plain"], "see shot.png now"))).toBe(false);
    expect(clipboardMayHoldImages(clipboard(["text/plain"], "a/b.png"))).toBe(false);
    expect(clipboardMayHoldImages(clipboard(["text/plain"], "line one\nshot.png"))).toBe(false);
    expect(clipboardMayHoldImages(clipboard(["text/plain"], "..."))).toBe(false);
  });

  it("is false for long text without looking at its lines", () => {
    const long = `${"a".repeat(5000)}.png`;
    expect(clipboardMayHoldImages(clipboard(["text/plain"], long))).toBe(false);
  });
});

describe("pasteImagesOrText", () => {
  it("inserts the references at the cursor as one undo step", async () => {
    const view = makeView("before\n");
    const deps = makeDeps({ kind: "inserted", markdown: "![](assets/a.png)", notices: [] });
    await pasteImagesOrText(view, deps, { text: "", sawImage: true });
    expect(deps.pasteImages).toHaveBeenCalledWith("buf-1");
    expect(view.state.doc.toString()).toBe("before\n![](assets/a.png)");
    undo(view);
    expect(view.state.doc.toString()).toBe("before\n");
    expect(deps.notices).toEqual([]);
  });

  it("inserts what it could and shows the notices for the rest", async () => {
    const view = makeView("");
    const deps = makeDeps({
      kind: "inserted",
      markdown: "![](assets/a.png)",
      notices: ["b.heic is not an image Writ can show"],
    });
    await pasteImagesOrText(view, deps, null);
    expect(view.state.doc.toString()).toBe("![](assets/a.png)");
    expect(deps.notices).toEqual(["b.heic is not an image Writ can show"]);
  });

  it("shows the notices and inserts nothing when every image is refused", async () => {
    const view = makeView("keep", 0);
    const deps = makeDeps({ kind: "refused", notices: ["a.png is over the 16 MB limit for images"] });
    await pasteImagesOrText(view, deps, { text: "a.png", sawImage: true });
    expect(view.state.doc.toString()).toBe("keep");
    expect(deps.notices).toEqual(["a.png is over the 16 MB limit for images"]);
  });

  it("pastes the text the event saw when the clipboard holds no images", async () => {
    const view = makeView("x", 1);
    const deps = makeDeps({ kind: "text" });
    await pasteImagesOrText(view, deps, { text: "notes.txt", sawImage: false });
    expect(view.state.doc.toString()).toBe("xnotes.txt");
    expect(deps.readText).not.toHaveBeenCalled();
  });

  it("reads the clipboard text for a menu paste with no images", async () => {
    const view = makeView("hello world", 6);
    view.dispatch({ selection: EditorSelection.single(6, 11) });
    const deps = makeDeps({ kind: "text" }, { readText: vi.fn(() => Promise.resolve("there")) });
    await pasteImagesOrText(view, deps, null);
    expect(view.state.doc.toString()).toBe("hello there");
  });

  it("leaves the selection alone when the clipboard is empty", async () => {
    const view = makeView("keep me", 0);
    view.dispatch({ selection: EditorSelection.single(0, 4) });
    const deps = makeDeps({ kind: "empty" });
    await pasteImagesOrText(view, deps, null);
    expect(view.state.doc.toString()).toBe("keep me");
    expect(deps.notices).toEqual([]);
  });

  it("says the clipboard changed when the image the event saw is gone", async () => {
    const view = makeView("");
    const deps = makeDeps({ kind: "empty" });
    await pasteImagesOrText(view, deps, { text: "", sawImage: true });
    expect(view.state.doc.toString()).toBe("");
    expect(deps.notices).toEqual([CLIPBOARD_CHANGED_NOTICE]);
  });

  it("inserts nothing into a tab the editor no longer holds", async () => {
    const view = makeView("");
    let current = "buf-1";
    const deps = makeDeps(
      { kind: "inserted", markdown: "![](assets/a.png)", notices: [] },
      {
        bufferId: () => current,
        pasteImages: vi.fn(() => {
          current = "buf-2";
          return Promise.resolve<ClipboardPasteResult>({
            kind: "inserted",
            markdown: "![](assets/a.png)",
            notices: [],
          });
        }),
      },
    );
    await pasteImagesOrText(view, deps, null);
    expect(view.state.doc.toString()).toBe("");
    expect(deps.notices).toEqual([TAB_CHANGED_NOTICE]);
  });

  it("logs a host failure and says the image was not pasted", async () => {
    const view = makeView("");
    const deps = makeDeps(new Error("clipboard unavailable"));
    await pasteImagesOrText(view, deps, { text: "", sawImage: true });
    expect(view.state.doc.toString()).toBe("");
    expect(deps.notices).toEqual([PASTE_FAILED_NOTICE]);
    expect(deps.logged).toEqual(["the clipboard could not be pasted into the file"]);
  });

  it("falls back to the text after a host failure on a text paste", async () => {
    const view = makeView("");
    const deps = makeDeps(new Error("clipboard unavailable"), {
      readText: vi.fn(() => Promise.resolve("plain")),
    });
    await pasteImagesOrText(view, deps, null);
    expect(view.state.doc.toString()).toBe("plain");
    expect(deps.notices).toEqual([]);
    expect(deps.logged).toHaveLength(1);
  });
});

describe("imagePaste", () => {
  function pasteEvent(data: DataTransfer): ClipboardEvent {
    const event = new Event("paste", { bubbles: true, cancelable: true }) as ClipboardEvent;
    Object.defineProperty(event, "clipboardData", { value: data });
    return event;
  }

  async function settle() {
    for (let tick = 0; tick < 8; tick++) await Promise.resolve();
  }

  it("takes a paste that holds files and inserts the references", async () => {
    const deps = makeDeps({ kind: "inserted", markdown: "![](assets/a.png)", notices: [] });
    const view = makeView("", 0, [imagePaste(deps)]);
    const event = pasteEvent(clipboard(["Files", "text/plain"], "a.png"));
    view.contentDOM.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    await settle();
    expect(view.state.doc.toString()).toBe("![](assets/a.png)");
  });

  it("leaves ordinary text to the editor's own paste", () => {
    const deps = makeDeps({ kind: "text" });
    const view = makeView("", 0, [imagePaste(deps)]);
    const event = pasteEvent(clipboard(["text/plain"], "hello world"));
    view.contentDOM.dispatchEvent(event);
    expect(deps.pasteImages).not.toHaveBeenCalled();
  });

  it("leaves every paste alone in a tab that takes no images", () => {
    const deps = makeDeps({ kind: "text" }, { acceptsImages: () => false });
    const view = makeView("", 0, [imagePaste(deps)]);
    const event = pasteEvent(clipboard(["image/png"]));
    view.contentDOM.dispatchEvent(event);
    expect(deps.pasteImages).not.toHaveBeenCalled();
  });

  it("leaves a paste alone in a read-only tab", () => {
    const deps = makeDeps({ kind: "text" });
    const view = makeView("", 0, [imagePaste(deps)], true);
    const event = pasteEvent(clipboard(["image/png"]));
    view.contentDOM.dispatchEvent(event);
    expect(deps.pasteImages).not.toHaveBeenCalled();
  });
});
