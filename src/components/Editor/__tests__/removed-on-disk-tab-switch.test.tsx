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
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
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
