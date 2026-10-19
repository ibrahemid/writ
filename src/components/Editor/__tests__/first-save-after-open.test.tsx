import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { ChangeSet, EditorSelection, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { WindowState } from "../../../stores/window/createWindowState";

// A large-tier note's first save after its open sends the changes since the
// text the load read (ADR-022). The fake backend holds the file's text and
// applies a change set only to the text its base digest names, as
// `save_buffer_changes` does.

const h = vi.hoisted(() => {
  const texts = new Map<string, string>();
  return {
    file: "",
    changedOnDisk: false,
    texts,
    digestOf(text: string): string {
      for (const [digest, known] of texts) if (known === text) return digest;
      const digest = `d${texts.size + 1}`;
      texts.set(digest, text);
      return digest;
    },
  };
});

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async () => h.file),
  noteDiskState: vi.fn(async () => ({
    state: "described",
    disk: { hash: h.digestOf(h.file), size: h.file.length, mtime_ms: 0 },
  })),
  saveBufferContent: vi.fn(async (_id: string, content: string) => {
    if (h.changedOnDisk) throw "ERR_FILE_CHANGED_ON_DISK: the file changed on disk";
    h.file = content;
    return h.digestOf(content);
  }),
  saveBufferChanges: vi.fn(async (_id: string, base: string, changes: unknown, length: number) => {
    if (h.texts.get(base) !== h.file) throw "ERR_CHANGES_STALE: the file moved on";
    const doc = ChangeSet.fromJSON(changes).apply(Text.of(h.file.split("\n")));
    if (doc.length !== length) throw "ERR_CHANGES_INVALID: wrong length";
    h.file = doc.sliceString(0);
    return h.digestOf(h.file);
  }),
  setBufferViewPosition: vi.fn(async () => {}),
  recordUnsavedNotes: vi.fn(async () => {}),
  discardUnsavedText: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async () => h.file),
  },
}));

import { noteDiskState, saveBufferChanges, saveBufferContent } from "../../../services/tauri";
import { debouncedSave, resetAutosave } from "../../../services/autosave";

const mockedWhole = vi.mocked(saveBufferContent);
const mockedChanges = vi.mocked(saveBufferChanges);
const mockedDiskState = vi.mocked(noteDiskState);

function big(): BufferDocument {
  return {
    id: "F1",
    title: "big.log",
    filename: "big.log",
    status: "active",
    language: null,
    source_path: "/files/big.log",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 6 * 1024 * 1024,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

function lines(count: number, word = "line"): string {
  return Array.from({ length: count }, (_, n) => `${word} ${n} · café`).join("\n");
}

async function flushMicrotasks(n = 30) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(): Promise<{ view: EditorView; win: WindowState }> {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buffer] = createSignal(big());
  let win: WindowState | null = null;
  function Capture() {
    win = useWindow();
    return null;
  }
  const { container } = render(() => (
    <WindowProvider windowId={9733}>
      <Capture />
      <EditorInstance buffer={buffer()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  const dom = container.querySelector(".cm-editor");
  if (!(dom instanceof HTMLElement)) throw new Error("no editor mounted");
  const view = EditorView.findFromDOM(dom);
  if (!view) throw new Error("no view behind the editor");
  if (!win) throw new Error("no window store");
  return { view, win };
}

function type(view: EditorView, at: number, text: string) {
  view.dispatch({
    changes: { from: at, insert: text },
    selection: EditorSelection.cursor(at + text.length),
    userEvent: "input.type",
  });
}

describe("EditorInstance: the first save after the open", () => {
  beforeEach(() => {
    resetAutosave();
    vi.clearAllMocks();
    h.file = "";
    h.changedOnDisk = false;
    h.texts.clear();
  });

  afterEach(() => {
    cleanup();
    resetAutosave();
    vi.restoreAllMocks();
  });

  it("sends only the change, and nothing turns the document into one string", async () => {
    h.file = lines(2_000);
    const { view, win } = await mount();
    expect(win.editor.largeFileMode()?.kind).toBe("LargeFile");

    type(view, 12, "typed");
    const toString = vi.spyOn(Text.prototype, "toString");
    const result = await win.editor.flushAutosave("F1");
    const materialised = toString.mock.calls.length;
    toString.mockRestore();

    expect(result.ok).toBe(true);
    expect(mockedWhole).not.toHaveBeenCalled();
    expect(mockedChanges).toHaveBeenCalledTimes(1);
    expect(mockedChanges.mock.calls[0][1]).toBe(h.digestOf(lines(2_000)));
    expect(JSON.stringify(mockedChanges.mock.calls[0][2]).length).toBeLessThan(100);
    expect(materialised, "the save turned the document into one string").toBe(0);
    expect(h.file).toBe(view.state.doc.sliceString(0));
  });

  it("sends the whole text into today's refusal when the file changed after the open", async () => {
    h.file = lines(2_000);
    const { view, win } = await mount();
    h.file = "what another program wrote";
    h.changedOnDisk = true;

    type(view, 12, "typed");
    const result = await win.editor.flushAutosave("F1");

    expect(result.ok).toBe(false);
    expect(String(result.failures[0]?.error)).toMatch(/^ERR_FILE_CHANGED_ON_DISK/);
    expect(mockedChanges).toHaveBeenCalledTimes(1);
    expect(mockedWhole).toHaveBeenCalledTimes(1);
    expect(mockedWhole.mock.calls[0][1]).toBe(view.state.doc.sliceString(0));
    expect(h.file).toBe("what another program wrote");
  });

  it("takes a reload's document and digest as the base of the next save", async () => {
    h.file = lines(2_000);
    const { view, win } = await mount();
    const reloaded = lines(2_000, "row");
    h.file = reloaded;
    win.editor.applyExternalContent("F1", reloaded);
    await flushMicrotasks();

    type(view, 3, "typed");
    expect((await win.editor.flushAutosave("F1")).ok).toBe(true);

    expect(mockedWhole).not.toHaveBeenCalled();
    expect(mockedChanges).toHaveBeenCalledTimes(1);
    expect(mockedChanges.mock.calls[0][1]).toBe(h.digestOf(reloaded));
    expect(h.file).toBe(view.state.doc.sliceString(0));
  });

  it("sends no changes against the text from before a reload whose open has not answered", async () => {
    h.file = lines(2_000);
    const { view, win } = await mount();
    mockedDiskState.mockImplementationOnce(() => new Promise<never>(() => {}));
    const reloaded = lines(2_000, "row");
    h.file = reloaded;
    win.editor.applyExternalContent("F1", reloaded);
    await flushMicrotasks();

    type(view, 3, "typed");
    expect((await win.editor.flushAutosave("F1")).ok).toBe(true);

    expect(mockedChanges).not.toHaveBeenCalled();
    expect(mockedWhole).toHaveBeenCalledTimes(1);
    expect(h.file).toBe(view.state.doc.sliceString(0));
  });

  it("sends the whole text after a load whose text is not the file's", async () => {
    h.file = lines(2_000);
    // Typing a failed write left, which the load shows instead of the file.
    debouncedSave("F1", "kept typing\nsecond line", 60_000);
    mockedWhole.mockRejectedValueOnce("ERR_WRITE_FAILED: the disk would not take the write");
    const { view, win } = await mount();
    expect(view.state.doc.sliceString(0)).toBe("kept typing\nsecond line");

    type(view, 4, "typed");
    expect((await win.editor.flushAutosave("F1")).ok).toBe(true);

    expect(mockedChanges).not.toHaveBeenCalled();
    expect(h.file).toBe(view.state.doc.sliceString(0));
  });
});
