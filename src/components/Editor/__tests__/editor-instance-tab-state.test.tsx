import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import { EditorSelection } from "@codemirror/state";
import { undo, undoDepth } from "@codemirror/commands";
import { foldCode, foldable, foldedRanges } from "@codemirror/language";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { WindowState } from "../../../stores/window/createWindowState";

// Switching tabs keeps each tab's editor: undo history, selection and scroll
// come back with it (EP-06), and a tab whose state is gone reopens where it
// was left.

const bufferContent = new Map<string, string>();
const positions: Array<[string, number, number]> = [];

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async (id: string, content: string) => {
    bufferContent.set(id, content);
    return null;
  }),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  setBufferViewPosition: vi.fn(async (id: string, cursor: number, top: number) => {
    positions.push([id, cursor, top]);
  }),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

function mockBuffer(id: string, extra: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.txt`,
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
    ...extra,
  };
}

async function flushMicrotasks(n = 30) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buf, setBuf] = createSignal(buffer);
  let win!: WindowState;
  function Capture() {
    win = useWindow();
    return null;
  }
  const result = render(() => (
    <WindowProvider windowId={9620}>
      <Capture />
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, setBuf, win: () => win };
}

function viewIn(container: HTMLElement): EditorView {
  const dom = container.querySelector(".cm-editor");
  if (!(dom instanceof HTMLElement)) throw new Error("no editor mounted");
  const view = EditorView.findFromDOM(dom);
  if (!view) throw new Error("no view behind the editor");
  return view;
}

function type(view: EditorView, at: number, text: string) {
  view.dispatch({
    changes: { from: at, insert: text },
    selection: EditorSelection.cursor(at + text.length),
    userEvent: "input.type",
  });
}

async function switchTo(setBuf: (b: BufferDocument) => void, buffer: BufferDocument) {
  setBuf(buffer);
  await flushMicrotasks();
}

describe("EditorInstance: tabs keep their place", () => {
  beforeEach(async () => {
    (await import("../../../services/autosave")).resetAutosave();
    bufferContent.clear();
    positions.length = 0;
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("keeps undo depth, selection and scroll across a switch away and back", async () => {
    bufferContent.set("A", "first line\nsecond line\n");
    bufferContent.set("B", "another file\n");
    const { container, setBuf } = await mount(mockBuffer("A"));
    let view = viewIn(container);
    type(view, 6, "typed ");
    type(view, 0, "more ");
    view.dispatch({ selection: EditorSelection.range(2, 9) });
    const depth = undoDepth(view.state);
    const scroll = vi.spyOn(view, "scrollSnapshot");
    expect(depth).toBeGreaterThan(0);

    await switchTo(setBuf, mockBuffer("B"));
    expect(scroll).toHaveBeenCalled();
    view = viewIn(container);
    expect(view.state.doc.toString()).toBe("another file\n");
    expect(undoDepth(view.state)).toBe(0);

    const dispatched: unknown[] = [];
    const original = view.dispatch.bind(view);
    vi.spyOn(view, "dispatch").mockImplementation((...args: Parameters<EditorView["dispatch"]>) => {
      dispatched.push(args[0]);
      original(...args);
    });
    await switchTo(setBuf, mockBuffer("A"));
    view = viewIn(container);

    expect(view.state.doc.toString()).toBe("more first typed line\nsecond line\n");
    expect(undoDepth(view.state)).toBe(depth);
    expect(view.state.selection.main.from).toBe(2);
    expect(view.state.selection.main.to).toBe(9);
    // The scroll snapshot taken on the way out is put back.
    expect(
      dispatched.some((spec) => {
        const effects = (spec as { effects?: unknown }).effects;
        return effects !== undefined && !Array.isArray(effects);
      }),
    ).toBe(true);

    undo(view);
    expect(view.state.doc.toString()).toBe("first typed line\nsecond line\n");
  });

  it("keeps a note's folds across a switch away and back", async () => {
    const note = "# Plan\n\nFirst part.\n\n# Later\n\nSecond part.\n";
    bufferContent.set("N", note);
    bufferContent.set("B", "another file\n");
    const markdownNote = mockBuffer("N", { title: "N.md", filename: "N.md", source_path: "/notes/N.md" });
    const { container, setBuf } = await mount(markdownNote);
    let view = viewIn(container);
    // The grammar arrives on its own chunk; folding reads its tree.
    await vi.waitFor(() => {
      expect(foldable(viewIn(container).state, 0, note.indexOf("\n"))).not.toBeNull();
    });
    view = viewIn(container);
    view.dispatch({ selection: EditorSelection.cursor(2) });
    expect(foldCode(view)).toBe(true);
    const folded = () => {
      const ranges: Array<[number, number]> = [];
      foldedRanges(viewIn(container).state).between(0, note.length, (from, to) => {
        ranges.push([from, to]);
      });
      return ranges;
    };
    const before = folded();
    expect(before).toEqual([[note.indexOf("\n"), note.indexOf("\n\n# Later")]]);

    await switchTo(setBuf, mockBuffer("B"));
    expect(folded()).toEqual([]);
    await switchTo(setBuf, markdownNote);
    expect(folded()).toEqual(before);
  });

  it("opens fresh when the file changed while the tab was behind another", async () => {
    bufferContent.set("A", "original\n");
    bufferContent.set("B", "other\n");
    const { container, setBuf } = await mount(mockBuffer("A"));
    type(viewIn(container), 0, "x");
    await switchTo(setBuf, mockBuffer("B"));

    bufferContent.set("A", "changed elsewhere\n");
    await switchTo(setBuf, mockBuffer("A"));

    const view = viewIn(container);
    expect(view.state.doc.toString()).toBe("changed elsewhere\n");
    expect(undoDepth(view.state)).toBe(0);
  });

  it("takes a reload of a tab behind another into its kept state, undoably", async () => {
    bufferContent.set("A", "one\ntwo\n");
    bufferContent.set("B", "other\n");
    const { container, setBuf, win } = await mount(mockBuffer("A"));
    type(viewIn(container), 0, "zero ");
    await switchTo(setBuf, mockBuffer("B"));
    const depth = undoDepth(viewIn(container).state);
    expect(depth).toBe(0);

    bufferContent.set("A", "zero one\ntwo\nthree\n");
    win().editor.requestExternalReload("A");
    await flushMicrotasks();
    await switchTo(setBuf, mockBuffer("A"));

    const view = viewIn(container);
    expect(view.state.doc.toString()).toBe("zero one\ntwo\nthree\n");
    undo(view);
    expect(view.state.doc.toString()).toBe("zero one\ntwo\n");
  });

  it("drops a closed tab's kept state", async () => {
    bufferContent.set("A", "alpha\n");
    bufferContent.set("B", "beta\n");
    const { container, setBuf, win } = await mount(mockBuffer("A"));
    type(viewIn(container), 0, "x");
    await switchTo(setBuf, mockBuffer("B"));

    win().editor.noteClosed("A");
    await switchTo(setBuf, mockBuffer("A"));

    expect(undoDepth(viewIn(container).state)).toBe(0);
  });

  it("keeps nothing for the tab in front when it closes before the next tab loads", async () => {
    bufferContent.set("A", "alpha\n");
    bufferContent.set("B", "beta\n");
    const { container, setBuf, win } = await mount(mockBuffer("A"));
    type(viewIn(container), 0, "x");

    win().editor.noteClosed("A");
    await switchTo(setBuf, mockBuffer("B"));
    await switchTo(setBuf, mockBuffer("A"));

    expect(undoDepth(viewIn(container).state)).toBe(0);
  });

  it("opens a tab with no kept state where its row says it was left", async () => {
    bufferContent.set("A", "0123456789\nabcdefghij\n");
    const { container } = await mount(mockBuffer("A", { cursor_pos: 13, scroll_pos: 11 }));
    expect(viewIn(container).state.selection.main.head).toBe(13);
  });

  it("holds a position past the end of a shorter file to its end", async () => {
    bufferContent.set("A", "short\n");
    const { container } = await mount(mockBuffer("A", { cursor_pos: 500, scroll_pos: 400 }));
    expect(viewIn(container).state.selection.main.head).toBe(6);
  });

  it("writes where the outgoing tab was left to its row", async () => {
    bufferContent.set("A", "first line\nsecond line\n");
    bufferContent.set("B", "other\n");
    const { container, setBuf } = await mount(mockBuffer("A"));
    viewIn(container).dispatch({ selection: EditorSelection.cursor(15) });

    await switchTo(setBuf, mockBuffer("B"));

    expect(positions).toContainEqual(["A", 15, 0]);
  });
});

describe("EditorInstance: the tier follows the document", () => {
  beforeEach(async () => {
    (await import("../../../services/autosave")).resetAutosave();
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("offers the large tier after a paste takes a note across 5 MiB, and switches on the person's word", async () => {
    bufferContent.set("A", "a small note\n");
    const { container, win } = await mount(mockBuffer("A"));
    const view = viewIn(container);
    expect(view.contentDOM.classList.contains("cm-lineWrapping")).toBe(true);

    view.dispatch({
      changes: { from: view.state.doc.length, insert: "pasted line of text\n".repeat(280_000) },
      userEvent: "input.paste",
    });

    expect(win().editor.tierOffer()).toEqual({ bufferId: "A", mode: { kind: "LargeFile" } });
    expect(win().editor.largeFileMode()).toBeNull();

    win().editor.acceptTierOffer();
    await flushMicrotasks();

    expect(win().editor.largeFileMode()).toEqual({ kind: "LargeFile" });
    expect(win().editor.tierOf("A")).toEqual({ kind: "LargeFile" });
    expect(undoDepth(view.state)).toBe(1);
    expect(view.contentDOM.classList.contains("cm-lineWrapping")).toBe(false);

    win().editor.setOverride("A", "wrap", true);
    await flushMicrotasks();
    expect(view.contentDOM.classList.contains("cm-lineWrapping")).toBe(true);
  });

  it("withdraws the offer when the paste is undone", async () => {
    bufferContent.set("A", "a small note\n");
    const { container, win } = await mount(mockBuffer("A"));
    const view = viewIn(container);
    view.dispatch({
      changes: { from: 0, insert: "pasted line of text\n".repeat(280_000) },
      userEvent: "input.paste",
    });
    expect(win().editor.tierOffer()?.mode.kind).toBe("LargeFile");

    undo(view);

    expect(win().editor.tierOffer()).toBeNull();
  });
});
