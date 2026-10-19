import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { Show, createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import { EditorSelection } from "@codemirror/state";
import { undo, undoDepth } from "@codemirror/commands";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { WindowState } from "../../../stores/window/createWindowState";
import type { PaneId } from "../../../lib/pane-layout";

// A tab switched back to keeps its editor without its file being read while
// the backend finds the file as Writ last read or wrote it (perf-15), and
// reads the file when it does not: a change no watcher reported.

// `record` is the backend's record of each file, which every read and write
// moves, as Rust's does.
const h = vi.hoisted(() => ({
  disk: new Map<string, string>(),
  record: new Map<string, string>(),
  openTabs: [] as BufferDocument[],
  unchanged: true,
  saveFails: false,
  read(id: string): string {
    const text = this.disk.get(id) ?? "";
    this.record.set(id, text);
    return text;
  },
}));

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => h.read(id)),
  saveBufferContent: vi.fn(async (id: string, content: string) => {
    if (h.saveFails) throw new Error("ERR_WRITE_FAILED: the disk is full");
    h.disk.set(id, content);
    h.record.set(id, content);
    return `digest-${content.length}`;
  }),
  noteDiskState: vi.fn(async (id: string) => ({
    state: "described",
    disk: { hash: `digest-${(h.disk.get(id) ?? "").length}`, size: 0, mtime_ms: 0 },
  })),
  noteFileUnchanged: vi.fn(async (id: string) => h.unchanged && h.record.get(id) === h.disk.get(id)),
  setBufferViewPosition: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => h.read(id)),
    activeTabs: () => h.openTabs,
  },
}));

function mockBuffer(id: string): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.txt`,
    status: "active",
    language: null,
    source_path: `/notes/${id}.txt`,
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
  };
}

async function flushMicrotasks(n = 40) {
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
    <WindowProvider windowId={9621}>
      <Capture />
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, setBuf, win: () => win };
}

async function mountPanes(left: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [leftBuf, setLeft] = createSignal(left);
  const [rightBuf, setRight] = createSignal<BufferDocument | null>(null);
  let win!: WindowState;
  function Capture() {
    win = useWindow();
    return null;
  }
  render(() => (
    <WindowProvider windowId={9622}>
      <Capture />
      <EditorInstance buffer={leftBuf()} pane="left" />
      <Show when={rightBuf()}>{(b) => <EditorInstance buffer={b()} pane="right" />}</Show>
    </WindowProvider>
  ));
  await flushMicrotasks();
  const view = (pane: PaneId): EditorView => {
    const found = win.editor.forPane(pane).getView();
    if (!found) throw new Error(`no editor in the ${pane} pane`);
    return found;
  };
  return { setLeft, setRight, win: () => win, view };
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

async function services() {
  const api = await import("../../../services/tauri");
  const { bufferRegistry } = await import("../../../stores/global/buffer-registry");
  return { api, readContent: vi.mocked(bufferRegistry.readContent) };
}

// Types into A, selects a range and switches to B, leaving A's save landed.
async function leaveEditedTab() {
  h.disk.set("A", "first line\nsecond line\n");
  h.disk.set("B", "another file\n");
  const mounted = await mount(mockBuffer("A"));
  const view = viewIn(mounted.container);
  type(view, 6, "typed ");
  view.dispatch({ selection: EditorSelection.range(2, 9) });
  const depth = undoDepth(view.state);
  const doc = view.state.doc;
  await switchTo(mounted.setBuf, mockBuffer("B"));
  return { ...mounted, depth, doc };
}

describe("EditorInstance: a tab switched back to", () => {
  beforeEach(async () => {
    (await import("../../../services/autosave")).resetAutosave();
    h.disk.clear();
    h.record.clear();
    h.openTabs.length = 0;
    h.unchanged = true;
    h.saveFails = false;
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("keeps its editor without reading or hashing its file when the file is as Writ left it", async () => {
    const { container, setBuf, depth, doc, win } = await leaveEditedTab();
    expect(win().editor.isDirty("A")).toBe(false);
    const { api, readContent } = await services();
    readContent.mockClear();
    vi.mocked(api.noteDiskState).mockClear();
    const whole = vi.spyOn(doc, "toString");

    await switchTo(setBuf, mockBuffer("A"));

    const view = viewIn(container);
    expect(readContent).not.toHaveBeenCalled();
    expect(api.noteDiskState).not.toHaveBeenCalled();
    expect(api.noteFileUnchanged).toHaveBeenCalledTimes(1);
    expect(api.noteFileUnchanged).toHaveBeenCalledWith("A");
    expect(whole).not.toHaveBeenCalled();
    expect(view.state.doc).toBe(doc);
    expect(undoDepth(view.state)).toBe(depth);
    expect(view.state.selection.main.from).toBe(2);
    expect(view.state.selection.main.to).toBe(9);
    expect(win().editor.isDirty("A")).toBe(false);
  });

  it("reads a change made on disk while it was away that no watcher reported", async () => {
    const { container, setBuf } = await leaveEditedTab();
    h.disk.set("A", "changed elsewhere\n");
    h.unchanged = false;
    const { readContent } = await services();
    readContent.mockClear();

    await switchTo(setBuf, mockBuffer("A"));

    const view = viewIn(container);
    expect(readContent).toHaveBeenCalledWith("A");
    expect(view.state.doc.toString()).toBe("changed elsewhere\n");
    expect(undoDepth(view.state)).toBe(0);
  });

  it("keeps its undo history when the file it reads holds the same text", async () => {
    const { container, setBuf, depth } = await leaveEditedTab();
    h.unchanged = false;

    await switchTo(setBuf, mockBuffer("A"));

    expect(undoDepth(viewIn(container).state)).toBe(depth);
  });

  it("reads its file when the check cannot be made", async () => {
    const { container, setBuf } = await leaveEditedTab();
    const { api, readContent } = await services();
    vi.mocked(api.noteFileUnchanged).mockRejectedValueOnce(new Error("no answer"));
    h.disk.set("A", "changed elsewhere\n");
    readContent.mockClear();

    await switchTo(setBuf, mockBuffer("A"));

    expect(readContent).toHaveBeenCalledWith("A");
    expect(viewIn(container).state.doc.toString()).toBe("changed elsewhere\n");
  });

  it("reads its file when it left with text its file does not hold", async () => {
    h.disk.set("A", "first line\n");
    h.disk.set("B", "another file\n");
    const { container, setBuf, win } = await mount(mockBuffer("A"));
    const { api, readContent } = await services();
    h.saveFails = true;
    type(viewIn(container), 0, "x");
    await switchTo(setBuf, mockBuffer("B"));
    expect(win().editor.isDirty("A")).toBe(true);
    readContent.mockClear();

    await switchTo(setBuf, mockBuffer("A"));

    expect(api.noteFileUnchanged).not.toHaveBeenCalled();
    expect(viewIn(container).state.doc.toString()).toBe("xfirst line\n");
  });

  it("reads clean again once typing after the switch back is undone", async () => {
    const { container, setBuf, win } = await leaveEditedTab();
    await switchTo(setBuf, mockBuffer("A"));
    const view = viewIn(container);

    type(view, 0, "z");
    expect(win().editor.isDirty("A")).toBe(true);
    undo(view);

    await vi.waitFor(() => expect(win().editor.isDirty("A")).toBe(false), { timeout: 700, interval: 20 });
  });

  it("comes back with a reload taken while it was away, reading nothing more", async () => {
    const { container, setBuf, win } = await leaveEditedTab();
    h.disk.set("A", "first typed line\nsecond line\nthird line\n");
    win().editor.requestExternalReload("A");
    await flushMicrotasks();
    const { readContent } = await services();
    expect(readContent).toHaveBeenCalledWith("A");
    readContent.mockClear();

    await switchTo(setBuf, mockBuffer("A"));

    const view = viewIn(container);
    expect(readContent).not.toHaveBeenCalled();
    expect(view.state.doc.toString()).toBe("first typed line\nsecond line\nthird line\n");
    undo(view);
    expect(view.state.doc.toString()).toBe("first typed line\nsecond line\n");
  });

  it("reads its file when the other pane read a change while it was away", async () => {
    const [a, b, c] = [mockBuffer("A"), mockBuffer("B"), mockBuffer("C")];
    h.openTabs.push(a, b, c);
    h.disk.set("A", "first line\n");
    h.disk.set("B", "another file\n");
    h.disk.set("C", "a third file\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.splitRight();
    panes.setRight(a);
    await flushMicrotasks();
    panes.win().tabs.moveToOtherPane("B", "left");
    panes.setRight(b);
    await flushMicrotasks();
    panes.win().tabs.selectIn("left", "C");
    await switchTo(panes.setLeft, c);

    h.disk.set("A", "changed elsewhere\n");
    await switchTo(panes.setLeft, a);
    expect(panes.view("left").state.doc.toString()).toBe("changed elsewhere\n");
    await switchTo(panes.setLeft, c);

    panes.win().tabs.selectIn("right", "A");
    await switchTo(panes.setRight, a);

    expect(panes.view("right").state.doc.toString()).toBe("changed elsewhere\n");
  });
});
