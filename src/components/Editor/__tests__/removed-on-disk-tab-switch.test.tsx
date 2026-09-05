import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";

const bufferContent = new Map<string, string>();
const deleted = new Set<string>();

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  // A write that lands recreates the file, which is what the backend's
  // NotFound pass-through does on main.
  saveBufferContent: vi.fn(async (id: string, content: string) => {
    bufferContent.set(id, content);
    deleted.delete(id);
  }),
  // What the backend answers for a note whose row names a file that is not
  // there. `no_file` is for a note that names none at all, and putting it here
  // would drive these tabs down a branch they can never reach.
  noteDiskState: vi.fn(async () => ({ state: "undescribed" })),
  // The one write the backend does not refuse for a note whose file is gone.
  restoreNoteFile: vi.fn(async (id: string, content: string) => {
    bufferContent.set(id, content);
    deleted.delete(id);
    return "disk-hash";
  }),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => {
      if (deleted.has(id)) throw new Error("io error: No such file or directory");
      return bufferContent.get(id) ?? "";
    }),
  },
}));

function mockBuffer(id: string, path: string | null): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.md`,
    status: "active",
    language: null,
    source_path: path,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

async function flush(n = 30) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

describe("a note whose file was deleted outside Writ", () => {
  beforeEach(() => {
    bufferContent.clear();
    deleted.clear();
    vi.clearAllMocks();
  });
  afterEach(() => cleanup());

  it("keeps its text across a tab switch", async () => {
    const EditorInstance = (await import("../EditorInstance")).default;

    bufferContent.set("A", "the only copy of this text");
    bufferContent.set("B", "other note");

    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [buf, setBuf] = createSignal(mockBuffer("A", "/notes/A.md"));
    const { container } = render(() => (
      <WindowProvider windowId={9101}>
        <Probe />
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));
    await flush();

    const text = () => container.querySelector(".cm-content")?.textContent ?? "";
    expect(text()).toContain("the only copy");

    // The file goes outside Writ. The tab is marked and keeps its text.
    deleted.add("A");
    win!.editor.markRemovedOnDisk("A");
    await flush();

    // The person keeps typing into the tab that is still on screen.
    const view = EditorView.findFromDOM(
      container.querySelector(".cm-editor") as HTMLElement,
    );
    expect(view).not.toBeNull();
    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " plus an unsaved edit" },
    });
    await flush();
    expect(text()).toContain("plus an unsaved edit");

    // A tab switch and back.
    setBuf(mockBuffer("B", "/notes/B.md"));
    await flush();
    setBuf(mockBuffer("A", "/notes/A.md"));
    await flush();

    expect(text()).toContain("the only copy");
    expect(text()).toContain("plus an unsaved edit");
  });

  it("writes its text back to its own path when asked to", async () => {
    const EditorInstance = (await import("../EditorInstance")).default;
    const { restoreNoteFile } = await import("../../../services/tauri");

    bufferContent.set("A", "the only copy of this text");
    bufferContent.set("B", "other note");

    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [buf, setBuf] = createSignal(mockBuffer("A", "/notes/A.md"));
    const { container } = render(() => (
      <WindowProvider windowId={9102}>
        <Probe />
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));
    await flush();

    deleted.add("A");
    win!.editor.markRemovedOnDisk("A");
    await flush();

    const view = EditorView.findFromDOM(
      container.querySelector(".cm-editor") as HTMLElement,
    );
    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " plus an unsaved edit" },
    });
    await flush();

    // Away and back first, so what is written is the text the store kept and
    // not whatever the view happened to survive with.
    setBuf(mockBuffer("B", "/notes/B.md"));
    await flush();
    setBuf(mockBuffer("A", "/notes/A.md"));
    await flush();

    const result = await win!.editor.restoreRemovedFile("A");
    await flush();

    expect(result.ok).toBe(true);
    expect(restoreNoteFile).toHaveBeenCalledWith(
      "A",
      "the only copy of this text plus an unsaved edit",
    );
    expect(bufferContent.get("A")).toBe("the only copy of this text plus an unsaved edit");
    expect(win!.editor.isRemovedOnDisk("A")).toBe(false);
  });

  it("keeps the mark when the write back fails", async () => {
    const EditorInstance = (await import("../EditorInstance")).default;
    const { restoreNoteFile } = await import("../../../services/tauri");
    vi.mocked(restoreNoteFile).mockRejectedValueOnce(
      new Error("ERR_FILE_MISSING: the folder is gone"),
    );

    bufferContent.set("A", "the only copy of this text");

    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [buf] = createSignal(mockBuffer("A", "/notes/A.md"));
    render(() => (
      <WindowProvider windowId={9103}>
        <Probe />
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));
    await flush();

    deleted.add("A");
    win!.editor.markRemovedOnDisk("A");
    await flush();

    const result = await win!.editor.restoreRemovedFile("A");
    expect(result.ok).toBe(false);
    // A restore that did not land leaves the note removed, so no later
    // keystroke recreates the file behind the person's back.
    expect(win!.editor.isRemovedOnDisk("A")).toBe(true);
    expect(win!.editor.textOfRemoved("A")).toBe("the only copy of this text");
  });
});

describe("a note whose file changed outside Writ", () => {
  beforeEach(() => {
    bufferContent.clear();
    deleted.clear();
    vi.clearAllMocks();
  });
  afterEach(() => cleanup());

  it("keeps its unsaved text across a tab switch", async () => {
    // The bar is up, so nothing may write, and the file holds the other
    // program's text. Reading that file back on the way in replaces the typing
    // the bar exists to protect, and the answer then sends the file its own
    // text and writes no copy of anything.
    const EditorInstance = (await import("../EditorInstance")).default;

    bufferContent.set("A", "as Writ opened it");
    bufferContent.set("B", "other note");

    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [buf, setBuf] = createSignal(mockBuffer("A", "/notes/A.md"));
    const { container } = render(() => (
      <WindowProvider windowId={9104}>
        <Probe />
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));
    await flush();

    const text = () => container.querySelector(".cm-content")?.textContent ?? "";

    const view = EditorView.findFromDOM(
      container.querySelector(".cm-editor") as HTMLElement,
    );
    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " plus my unsaved work" },
    });
    await flush();

    // Another program rewrites the file. The tab is dirty, so the question is
    // asked rather than the file being taken.
    bufferContent.set("A", "written by another program");
    win!.editor.recordFileEvent("A", "modified");
    await flush();
    expect(win!.editor.isFileChangedOnDisk("A")).toBe(true);

    setBuf(mockBuffer("B", "/notes/B.md"));
    await flush();
    setBuf(mockBuffer("A", "/notes/A.md"));
    await flush();

    expect(text()).toContain("plus my unsaved work");
    expect(text()).not.toContain("written by another program");
    expect(win!.editor.isFileChangedOnDisk("A")).toBe(true);
  });

  it("keeps what was typed after its file came back holding something else", async () => {
    // The kept copy is refreshed on every hold, not written once when the file
    // went. Left at what the note held the moment it was deleted, a switch
    // reverts everything typed under the bar that replaced the deletion.
    const EditorInstance = (await import("../EditorInstance")).default;

    bufferContent.set("A", "as Writ opened it");
    bufferContent.set("B", "other note");

    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [buf, setBuf] = createSignal(mockBuffer("A", "/notes/A.md"));
    const { container } = render(() => (
      <WindowProvider windowId={9105}>
        <Probe />
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));
    await flush();

    const text = () => container.querySelector(".cm-content")?.textContent ?? "";
    const view = EditorView.findFromDOM(
      container.querySelector(".cm-editor") as HTMLElement,
    );

    deleted.add("A");
    win!.editor.markRemovedOnDisk("A");
    await flush();

    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " typed while gone" },
    });
    await flush();
    win!.editor.keepTextOfRemoved("A", view!.state.doc.toString());

    // The file comes back at the same path holding something else, and the tab
    // is dirty, so the question replaces the deletion.
    deleted.delete("A");
    bufferContent.set("A", "what came back");
    win!.editor.recordFileEvent("A", "modified");
    await flush();
    expect(win!.editor.isFileChangedOnDisk("A")).toBe(true);

    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " typed under the bar" },
    });
    await flush();

    setBuf(mockBuffer("B", "/notes/B.md"));
    await flush();
    setBuf(mockBuffer("A", "/notes/A.md"));
    await flush();

    expect(text()).toContain("typed while gone");
    expect(text()).toContain("typed under the bar");
    expect(text()).not.toContain("what came back");
  });

  it("is asked about rather than reloaded when its file comes back", async () => {
    // The whole route, and not `recordFileEvent` by hand: the watcher's mark
    // arrives at `handleExternalEdit`, which asks the store whether the tab
    // holds anything before it decides. A tab switched away and back has had
    // its note opened a second time, and that open's answer is what this
    // decision rests on.
    const EditorInstance = (await import("../EditorInstance")).default;
    const { handleExternalEdit } = await import("../../../services/external-edit");
    const { createExternalEditDeps } = await import("../../../lib/external-edit-deps");
    const { peekUnsavedContent } = await import("../../../services/autosave");

    bufferContent.set("A", "as Writ opened it");
    bufferContent.set("B", "other note");
    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [buf, setBuf] = createSignal(mockBuffer("A", "/notes/A.md"));
    const { container } = render(() => (
      <WindowProvider windowId={9107}>
        <Probe />
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));
    await flush();
    const text = () => container.querySelector(".cm-content")?.textContent ?? "";

    const view = EditorView.findFromDOM(
      container.querySelector(".cm-editor") as HTMLElement,
    );
    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " plus my unsaved work" },
    });
    await flush();

    deleted.add("A");
    win!.editor.markRemovedOnDisk("A");
    await flush();

    setBuf(mockBuffer("B", "/notes/B.md"));
    await flush();
    setBuf(mockBuffer("A", "/notes/A.md"));
    await flush(80);

    // The file is back at the same path holding another program's text.
    deleted.delete("A");
    bufferContent.set("A", "written by another program");
    const deps = createExternalEditDeps({
      editor: win!.editor,
      openBuffers: () => [{ id: "A", title: "A", filename: "A.md" }],
      refreshBuffer: async () => {},
      forgetSaveStatus: () => {},
    });
    await handleExternalEdit(
      { bufferId: "A", change: "modified", path: "/notes/A.md", diskHash: "back" },
      deps,
    );
    await flush(80);

    expect(win!.editor.isFileChangedOnDisk("A")).toBe(true);
    expect(text()).toContain("plus my unsaved work");
    expect(text()).not.toContain("written by another program");
    expect(peekUnsavedContent("A")).toContain("plus my unsaved work");
  });
});
