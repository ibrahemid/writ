import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { Show, createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../../types/buffer";
import type { ExternalEditPayload } from "../../../services/external-edit";

// What the editor does with typing when the view it was typed into goes away
// or is replaced: the last tab closing, a click on a file whose bytes are not
// here, a crash screen, a tab switch in a large file. The stores are the real
// ones and only the IPC is mocked, because each of these is a composition of
// the tab store, the registry, autosave and the editor, and every suite that
// mocks its neighbours passes while the composition loses the text.

const h = vi.hoisted(() => ({
  content: new Map<string, string>(),
  saves: [] as Array<[string, string]>,
  readFails: new Map<string, unknown>(),
  gates: new Map<string, Promise<void>>(),
}));

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => {
    const gate = h.gates.get(id);
    if (gate) await gate;
    if (h.readFails.has(id)) throw h.readFails.get(id);
    return h.content.get(id) ?? "";
  }),
  saveBufferContent: vi.fn(async (id: string, content: string) => {
    h.saves.push([id, content]);
    h.content.set(id, content);
    return `hash-${content.length}`;
  }),
  noteDiskState: vi.fn(async () => ({
    state: "described",
    disk: { hash: "disk-hash", size: 17, mtime_ms: 0 },
  })),
  recordUnsavedNotes: vi.fn(async () => {}),
  restoreNoteFile: vi.fn(async () => "x"),
  closeBuffer: vi.fn(async () => {}),
  closeBuffers: vi.fn(async () => {}),
  previewClose: vi.fn(async () => {}),
  materialiseNote: vi.fn(async () => {}),
  cancelMaterialiseNote: vi.fn(async () => {}),
  showNoteInFileManager: vi.fn(async () => {}),
  openFile: vi.fn(async (path: string) => ({
    doc: null,
    mode: { kind: "NotDownloaded", path, provider: "iCloud Drive" },
    size_bytes: 0,
  })),
}));

vi.mock("../../../services/events", () => ({
  onEvent: vi.fn(async () => () => {}),
  emitFrontendReady: vi.fn(async () => {}),
}));

function mockBuffer(id: string, sizeBytes = 17): BufferDocument {
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
    size_bytes: sizeBytes,
    line_ending: "lf",
  };
}

async function flush(count = 40): Promise<void> {
  for (let i = 0; i < count; i += 1) await Promise.resolve();
}

function viewIn(container: HTMLElement): EditorView | null {
  const element = container.querySelector(".cm-editor");
  return element ? EditorView.findFromDOM(element as HTMLElement) : null;
}

// A window showing the editor for its active tab, with no tab open yet. With
// `withRemovedBar`, the deletion bar sits above the editor as it does in the
// editor area.
async function mountWindow(windowId: number, withRemovedBar = false) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const RemovedOnDiskBar = (await import("../RemovedOnDiskBar")).default;
  const WindowProvider = (await import("../../WindowProvider/WindowProvider")).default;
  const { useWindow } = await import("../../WindowProvider/WindowProvider");
  const { useActiveBuffer } = await import("../../../lib/use-active-buffer");
  const { bufferRegistry } = await import("../../../stores/global/buffer-registry");

  let win: ReturnType<typeof useWindow> | null = null;
  function Host() {
    win = useWindow();
    const active = useActiveBuffer();
    return (
      <>
        <Show when={withRemovedBar}>
          <RemovedOnDiskBar noteId={active()?.id ?? null} />
        </Show>
        <Show when={active()}>{(b) => <EditorInstance buffer={b()} />}</Show>
      </>
    );
  }
  const screen = render(() => (
    <WindowProvider windowId={windowId}>
      <Host />
    </WindowProvider>
  ));
  return { win: win!, bufferRegistry, screen };
}

async function mountWith(id: string, windowId: number, sizeBytes = 17) {
  const { bufferRegistry } = await import("../../../stores/global/buffer-registry");
  bufferRegistry.registerOpenResult({
    doc: mockBuffer(id, sizeBytes),
    mode: { kind: "Normal" },
    size_bytes: sizeBytes,
  });
  const { win, screen } = await mountWindow(windowId);
  win.tabs.setActiveTabId(id);
  await flush();
  return { win, view: viewIn(screen.container), bufferRegistry, screen };
}

describe("typing reaches its file when the view goes away", () => {
  beforeEach(async () => {
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
    h.content.clear();
    h.saves.length = 0;
    h.readFails.clear();
    h.gates.clear();
    vi.clearAllMocks();
  });
  afterEach(async () => {
    cleanup();
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
  });

  it("closing the last tab right after typing writes the typing", async () => {
    h.content.set("L1", "as Writ opened it");
    const { win, view } = await mountWith("L1", 9501);
    expect(view!.state.doc.toString()).toBe("as Writ opened it");

    view!.dispatch({ changes: { from: view!.state.doc.length, insert: " and my last words" } });
    await flush();

    await win.tabs.closeTab("L1");
    await flush();

    expect(h.saves).toContainEqual(["L1", "as Writ opened it and my last words"]);
    expect(h.content.get("L1")).toBe("as Writ opened it and my last words");
  });

  it("opening a file whose bytes are not here right after typing writes the typing", async () => {
    h.content.set("L2", "first draft");
    const { win, view } = await mountWith("L2", 9502);
    view!.dispatch({ changes: { from: view!.state.doc.length, insert: " typed just now" } });
    await flush();

    await win.tabs.openFile("/notes/in-the-cloud.txt");
    await flush();

    expect(h.saves).toContainEqual(["L2", "first draft typed just now"]);
    expect(h.content.get("L2")).toBe("first draft typed just now");
  });

  it("the outgoing view of a switch takes no typing between the flush and the swap", async () => {
    h.content.set("G3", "log line one\n");
    h.content.set("G4", "the other note");
    const { win, view: oldView, bufferRegistry } = await mountWith("G3", 9510, 6 * 1024 * 1024);
    bufferRegistry.registerOpenResult({ doc: mockBuffer("G4"), mode: { kind: "Normal" }, size_bytes: 17 });
    expect(oldView!.state.readOnly).toBe(false);

    let release: () => void = () => {};
    h.gates.set("G4", new Promise<void>((resolve) => { release = resolve; }));
    win.tabs.setActiveTabId("G4");
    await flush();

    expect(oldView!.state.readOnly).toBe(true);
    expect(oldView!.state.facet(EditorView.editable)).toBe(false);
    release();
    await flush();
  });

  it("a change that reaches the outgoing view of a large file lands in that file", async () => {
    h.content.set("G1", "log line one\n");
    h.content.set("G2", "the other note");
    const { win, view: oldView, bufferRegistry } = await mountWith("G1", 9504, 6 * 1024 * 1024);
    bufferRegistry.registerOpenResult({ doc: mockBuffer("G2"), mode: { kind: "Normal" }, size_bytes: 17 });

    let release: () => void = () => {};
    h.gates.set("G2", new Promise<void>((resolve) => { release = resolve; }));
    win.tabs.setActiveTabId("G2");
    await flush();

    oldView!.dispatch({ changes: { from: oldView!.state.doc.length, insert: "typed while switching" } });
    await flush();
    release();
    await flush();
    await new Promise((resolve) => setTimeout(resolve, 2300));
    await flush();

    expect(h.saves).not.toContainEqual(["G1", "the other note"]);
    expect(h.saves).toContainEqual(["G1", "log line one\ntyped while switching"]);
    expect(h.content.get("G2")).toBe("the other note");
  });
});

describe("a file Writ could not read", () => {
  beforeEach(async () => {
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
    h.content.clear();
    h.saves.length = 0;
    h.readFails.clear();
    h.gates.clear();
    vi.clearAllMocks();
  });
  afterEach(async () => {
    cleanup();
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
  });

  it("never opens as a writable empty document", async () => {
    h.content.set("L3", "café bytes Writ could not decode");
    h.readFails.set("L3", "ERR_READ_NOT_UTF8: stream did not contain valid UTF-8");
    const { win, view, screen } = await mountWith("L3", 9503);

    expect(view).toBeNull();
    expect(win.editor.getView()).toBeNull();
    expect(win.editor.readFailure()).toEqual({ bufferId: "L3", code: "ERR_READ_NOT_UTF8" });
    expect(screen.getByRole("alert").textContent).toContain("not UTF-8 text");

    await win.editor.flushAutosave("L3");
    await win.editor.saveActiveBuffer();
    expect(h.saves).toEqual([]);
    expect(h.content.get("L3")).toBe("café bytes Writ could not decode");
  });

  it("opens a file that is not UTF-8 into a tab showing the failure", async () => {
    const api = await import("../../../services/tauri");
    // The open answers with a note and only its read fails: an open that
    // rejected instead is swallowed by the file tree, and the click did nothing.
    vi.mocked(api.openFile).mockResolvedValueOnce({
      doc: mockBuffer("L8"),
      mode: { kind: "Normal" },
      size_bytes: 17,
    });
    h.readFails.set("L8", "ERR_READ_NOT_UTF8: stream did not contain valid UTF-8");
    const { win, screen } = await mountWindow(9513);

    const doc = await win.tabs.openFile("/notes/L8.txt");
    await flush();

    expect(doc?.id).toBe("L8");
    expect(win.tabs.activeTabId()).toBe("L8");
    expect(viewIn(screen.container)).toBeNull();
    expect(win.editor.getView()).toBeNull();
    expect(win.editor.readFailure()).toEqual({ bufferId: "L8", code: "ERR_READ_NOT_UTF8" });
    expect(screen.getByRole("alert").textContent).toContain("not UTF-8 text");

    await win.editor.saveActiveBuffer();
    expect(h.saves).toEqual([]);
  });

  it("shows the failure for a rejection that carries no code", async () => {
    h.readFails.set("L4", new Error("stream did not contain valid UTF-8"));
    const { win, view, screen } = await mountWith("L4", 9505);

    expect(view).toBeNull();
    expect(win.editor.readFailure()).toEqual({ bufferId: "L4", code: "ERR_READ_FAILED" });
    expect(screen.getByRole("alert")).toBeTruthy();
  });

  it("opens the file on Try again once it can be read", async () => {
    h.content.set("L5", "readable now");
    h.readFails.set("L5", "ERR_READ_FILE_IN_USE: the process cannot access the file");
    const { win, screen } = await mountWith("L5", 9506);
    expect(viewIn(screen.container)).toBeNull();

    h.readFails.delete("L5");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await flush();

    const view = viewIn(screen.container);
    expect(view!.state.doc.toString()).toBe("readable now");
    expect(win.editor.readFailure()).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows the file in the file manager", async () => {
    const { showInFileManagerLabel } = await import("../../../lib/note-actions");
    const api = await import("../../../services/tauri");
    h.readFails.set("L6", "ERR_READ_PERMISSION_DENIED: permission denied");
    const { screen } = await mountWith("L6", 9507);

    fireEvent.click(screen.getByRole("button", { name: showInFileManagerLabel() }));
    await flush();

    expect(vi.mocked(api.showNoteInFileManager)).toHaveBeenCalledWith("L6");
  });

  it("closes the tab from the failure state", async () => {
    h.readFails.set("L7", "ERR_READ_FAILED: io error");
    const { win, screen, bufferRegistry } = await mountWith("L7", 9508);

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await flush();

    expect(win.tabs.activeTabId()).not.toBe("L7");
    expect(bufferRegistry.activeTabs().some((b) => b.id === "L7")).toBe(false);
    expect(win.editor.readFailure()).toBeNull();
    expect(h.saves).toEqual([]);
  });

  it("keeps showing the failure after its file changes on disk", async () => {
    const api = await import("../../../services/tauri");
    const { handleExternalEdit } = await import("../../../services/external-edit");
    const { createExternalEditDeps } = await import("../../../lib/external-edit-deps");
    h.content.set("L9", "the note before it");
    const { win, screen, bufferRegistry } = await mountWith("L9", 9514);
    vi.mocked(api.openFile).mockResolvedValueOnce({
      doc: mockBuffer("L10"),
      mode: { kind: "Normal" },
      size_bytes: 17,
    });
    h.content.set("L10", "cp1252 bytes");
    h.readFails.set("L10", "ERR_READ_NOT_UTF8: stream did not contain valid UTF-8");
    await win.tabs.openFile("/notes/L10.txt");
    await flush();
    expect(win.editor.readFailure()).toEqual({ bufferId: "L10", code: "ERR_READ_NOT_UTF8" });

    const deps = createExternalEditDeps({
      editor: win.editor,
      openBuffers: () => bufferRegistry.buffers(),
      refreshBuffer: async () => {},
      forgetSaveStatus: () => {},
    });
    const modified: ExternalEditPayload = {
      bufferId: "L10",
      change: "modified",
      path: "/notes/L10.txt",
      newPath: null,
      diskHash: "another program's bytes",
    };
    await handleExternalEdit(modified, deps);
    await flush();
    expect(win.editor.isFileChangedOnDisk("L10")).toBe(true);

    const showsOnlyTheFailure = () => {
      expect(viewIn(screen.container)).toBeNull();
      expect(win.editor.getView()).toBeNull();
      expect(win.editor.readFailure()).toEqual({ bufferId: "L10", code: "ERR_READ_NOT_UTF8" });
      expect(screen.getByRole("alert").textContent).toContain("not UTF-8 text");
      expect(screen.queryByText("This file changed on disk.")).toBeNull();
    };
    showsOnlyTheFailure();

    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await flush();
    showsOnlyTheFailure();

    win.tabs.setActiveTabId("L9");
    await flush();
    win.tabs.setActiveTabId("L10");
    await flush();
    showsOnlyTheFailure();

    await handleExternalEdit({ ...modified, diskHash: "a third version" }, deps);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await flush();
    showsOnlyTheFailure();

    await win.editor.saveActiveBuffer();
    await win.editor.flushAutosave();
    expect(h.saves).toEqual([]);
    expect(h.content.get("L10")).toBe("cp1252 bytes");
  });

  it("opens a note whose file is gone on its kept text, or on the failure when nothing is kept", async () => {
    const api = await import("../../../services/tauri");
    const { showInFileManagerLabel } = await import("../../../lib/note-actions");
    h.content.set("L11", "the note before it");
    const { win, screen, bufferRegistry } = await mountWith("L11", 9515);
    bufferRegistry.registerOpenResult({ doc: mockBuffer("L12"), mode: { kind: "Normal" }, size_bytes: 17 });
    bufferRegistry.registerOpenResult({ doc: mockBuffer("L13"), mode: { kind: "Normal" }, size_bytes: 17 });
    win.editor.markRemovedOnDisk("L12");
    win.editor.markRemovedOnDisk("L13", "typed before the deletion");
    h.readFails.set("L12", "ERR_READ_FILE_MISSING: gone");
    h.readFails.set("L13", "ERR_READ_FILE_MISSING: gone");

    win.tabs.setActiveTabId("L12");
    await flush();

    expect(viewIn(screen.container)).toBeNull();
    expect(win.editor.getView()).toBeNull();
    expect(win.editor.readFailure()).toEqual({ bufferId: "L12", code: "ERR_READ_FILE_MISSING" });
    expect(screen.getByRole("alert").textContent).toContain("it is no longer there.");
    expect(screen.queryByRole("button", { name: showInFileManagerLabel() })).toBeNull();
    expect(win.editor.savesAreHeld("L12")).toBe(true);
    await win.editor.saveActiveBuffer();
    await win.editor.flushAutosave();
    expect(h.saves).toEqual([]);
    expect(vi.mocked(api.restoreNoteFile)).not.toHaveBeenCalled();

    win.tabs.setActiveTabId("L13");
    await flush();

    const view = viewIn(screen.container);
    expect(view!.state.doc.toString()).toBe("typed before the deletion");
    expect(win.editor.readFailure()).toBeNull();
    expect(win.editor.savesAreHeld("L13")).toBe(true);
    view!.dispatch({ changes: { from: 0, insert: "more " } });
    await flush();
    await win.editor.flushAutosave();
    expect(h.saves).toEqual([]);
    expect(vi.mocked(api.restoreNoteFile)).not.toHaveBeenCalled();
  });

  it("shows no deletion bar over the failure when its file is then removed", async () => {
    const api = await import("../../../services/tauri");
    const { handleExternalEdit } = await import("../../../services/external-edit");
    const { createExternalEditDeps } = await import("../../../lib/external-edit-deps");
    vi.mocked(api.openFile).mockResolvedValueOnce({
      doc: mockBuffer("L14"),
      mode: { kind: "Normal" },
      size_bytes: 17,
    });
    h.readFails.set("L14", "ERR_READ_NOT_UTF8: stream did not contain valid UTF-8");
    const { win, screen, bufferRegistry } = await mountWindow(9516, true);
    await win.tabs.openFile("/notes/L14.txt");
    await flush();
    expect(win.editor.readFailure()).toEqual({ bufferId: "L14", code: "ERR_READ_NOT_UTF8" });

    const deps = createExternalEditDeps({
      editor: win.editor,
      openBuffers: () => bufferRegistry.buffers(),
      refreshBuffer: async () => {},
      forgetSaveStatus: () => {},
    });
    await handleExternalEdit({ bufferId: "L14", change: "removed", path: "/notes/L14.txt" }, deps);
    await flush();
    expect(win.editor.isRemovedOnDisk("L14")).toBe(true);

    const showsOnlyTheFailure = (code: string, reason: string) => {
      expect(viewIn(screen.container)).toBeNull();
      expect(win.editor.readFailure()).toEqual({ bufferId: "L14", code });
      expect(screen.queryByText(/was deleted/)).toBeNull();
      expect(screen.queryByRole("button", { name: "Put the file back" })).toBeNull();
      expect(screen.getAllByRole("alert")).toHaveLength(1);
      expect(screen.getByRole("alert").textContent).toContain(reason);
    };
    showsOnlyTheFailure("ERR_READ_NOT_UTF8", "not UTF-8 text");

    h.readFails.set("L14", "ERR_READ_FILE_MISSING: gone");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await flush();
    showsOnlyTheFailure("ERR_READ_FILE_MISSING", "it is no longer there.");
    expect(h.saves).toEqual([]);
    expect(vi.mocked(api.restoreNoteFile)).not.toHaveBeenCalled();
  });
});

describe("loads that overlap", () => {
  beforeEach(async () => {
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
    h.content.clear();
    h.saves.length = 0;
    h.readFails.clear();
    h.gates.clear();
    vi.clearAllMocks();
  });
  afterEach(async () => {
    cleanup();
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
  });

  it("a read that fails after a later switch leaves the later note on screen", async () => {
    h.content.set("O1", "the first note");
    h.content.set("O3", "the note switched to last");
    const { win, screen, bufferRegistry } = await mountWith("O1", 9511);
    bufferRegistry.registerOpenResult({ doc: mockBuffer("O2"), mode: { kind: "Normal" }, size_bytes: 17 });
    bufferRegistry.registerOpenResult({ doc: mockBuffer("O3"), mode: { kind: "Normal" }, size_bytes: 17 });

    let release: () => void = () => {};
    h.gates.set("O2", new Promise<void>((resolve) => { release = resolve; }));
    h.readFails.set("O2", "ERR_READ_FILE_IN_USE: locked");
    win.tabs.setActiveTabId("O2");
    await flush();
    win.tabs.setActiveTabId("O3");
    await flush();

    release();
    await flush();

    expect(viewIn(screen.container)!.state.doc.toString()).toBe("the note switched to last");
    expect(win.editor.currentBufferId()).toBe("O3");
    expect(win.editor.readFailure()).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("the view's text is never kept as the text of a note still loading", async () => {
    // A note whose file is gone and that has nothing kept reads its file,
    // and a switch away while that read is out finds the editor still
    // showing the note before it.
    h.content.set("H1", "the note on screen");
    h.content.set("H3", "the note switched to last");
    const { win, bufferRegistry } = await mountWith("H1", 9512);
    bufferRegistry.registerOpenResult({ doc: mockBuffer("H2"), mode: { kind: "Normal" }, size_bytes: 17 });
    bufferRegistry.registerOpenResult({ doc: mockBuffer("H3"), mode: { kind: "Normal" }, size_bytes: 17 });
    win.editor.markRemovedOnDisk("H2");

    let release: () => void = () => {};
    h.gates.set("H2", new Promise<void>((resolve) => { release = resolve; }));
    h.readFails.set("H2", "ERR_READ_FILE_MISSING: gone");
    win.tabs.setActiveTabId("H2");
    await flush();
    win.tabs.setActiveTabId("H3");
    await flush();
    release();
    await flush();

    expect(win.editor.textOfRemoved("H2")).toBeUndefined();
  });
});

describe("a crash screen in front of the editor", () => {
  beforeEach(async () => {
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
    h.content.clear();
    h.saves.length = 0;
    h.readFails.clear();
    h.gates.clear();
    vi.clearAllMocks();
  });
  afterEach(async () => {
    cleanup();
    const { resetAutosave } = await import("../../../services/autosave");
    resetAutosave();
  });

  it("keeps the typing through a reset", async () => {
    const EditorInstance = (await import("../EditorInstance")).default;
    const ErrorBoundary = (await import("../../ErrorBoundary/ErrorBoundary")).default;
    const WindowProvider = (await import("../../WindowProvider/WindowProvider")).default;
    const { useWindow } = await import("../../WindowProvider/WindowProvider");
    const { useActiveBuffer } = await import("../../../lib/use-active-buffer");
    const { bufferRegistry } = await import("../../../stores/global/buffer-registry");

    h.content.set("E1", "as Writ opened it");
    bufferRegistry.registerOpenResult({ doc: mockBuffer("E1"), mode: { kind: "Normal" }, size_bytes: 17 });

    const [broken, setBroken] = createSignal(false);
    function explode(): never {
      throw new Error("a component threw");
    }
    let win: ReturnType<typeof useWindow> | null = null;
    function Host() {
      win = useWindow();
      const active = useActiveBuffer();
      return (
        <>
          {broken() ? explode() : null}
          <Show when={active()}>{(b) => <EditorInstance buffer={b()} />}</Show>
        </>
      );
    }
    const screen = render(() => (
      <WindowProvider windowId={9509}>
        <ErrorBoundary>
          <Host />
        </ErrorBoundary>
      </WindowProvider>
    ));
    win!.tabs.setActiveTabId("E1");
    await flush();

    const view = viewIn(screen.container)!;
    view.dispatch({ changes: { from: view.state.doc.length, insert: " typed before the crash" } });
    await flush();

    setBroken(true);
    await flush();
    expect(viewIn(screen.container)).toBeNull();

    setBroken(false);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await flush();

    const remounted = viewIn(screen.container)!;
    expect(remounted.state.doc.toString()).toBe("as Writ opened it typed before the crash");
    await win!.editor.flushAutosave();
    expect(h.content.get("E1")).toBe("as Writ opened it typed before the crash");
  });
});
