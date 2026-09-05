import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal, Show } from "solid-js";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";

const bufferContent = new Map<string, string>();

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async (id: string, content: string) => {
    bufferContent.set(id, content);
  }),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  recordUnsavedNotes: vi.fn(async () => {}),
  restoreNoteFile: vi.fn(async (id: string, content: string) => {
    bufferContent.set(id, content);
    return "disk-hash";
  }),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

function mockBuffer(id: string): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.md`,
    status: "active",
    language: null,
    source_path: `/notes/${id}.md`,
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

async function flush(count = 30): Promise<void> {
  for (let i = 0; i < count; i += 1) await Promise.resolve();
}

/**
 * The last tab of a note under the bar, closed.
 *
 * The tab store selects the surviving tab before it closes the buffer, and
 * with no survivor that selection is null, which unmounts the editor. So the
 * view is torn down before the close path has taken the text: the mount here
 * is a `Show` over the active buffer, the same shape `PreviewLayout` uses, and
 * the close is performed in the order `closeTab` performs it.
 */
describe("closing the last tab of a note whose file changed outside Writ", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });
  afterEach(() => cleanup());

  it("hands the typing nobody has answered for to the recovery snapshot", async () => {
    const EditorInstance = (await import("../EditorInstance")).default;
    const { keepUnsavedForRecovery, collectUnsavedContent } = await import(
      "../../../services/autosave"
    );
    const { recordUnsavedNotes } = await import("../../../services/tauri");

    bufferContent.set("A", "as Writ opened it");
    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [active, setActive] = createSignal<BufferDocument | null>(mockBuffer("A"));
    const { container } = render(() => (
      <WindowProvider windowId={9401}>
        <Probe />
        <Show when={active()}>{(buffer) => <EditorInstance buffer={buffer()} />}</Show>
      </WindowProvider>
    ));
    await flush();

    const view = EditorView.findFromDOM(
      container.querySelector(".cm-editor") as HTMLElement,
    );
    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " plus my unsaved work" },
    });
    await flush();

    bufferContent.set("A", "written by another program");
    win!.editor.recordFileEvent("A", "modified");
    await flush();

    // The last tab goes. The active buffer is null before the close runs, so
    // the editor unmounts first.
    setActive(null);
    await flush();

    const flushed = await win!.editor.flushAutosave("A");
    expect(flushed.ok).toBe(true);
    expect(collectUnsavedContent()).toEqual([
      { id: "A", content: "as Writ opened it plus my unsaved work" },
    ]);

    win!.editor.noteClosed("A");
    await keepUnsavedForRecovery("A");

    expect(vi.mocked(recordUnsavedNotes)).toHaveBeenCalledWith([
      { id: "A", content: "as Writ opened it plus my unsaved work" },
    ]);
    expect(bufferContent.get("A")).toBe("written by another program");
  });

  it("hands over the text of a note whose file was deleted", async () => {
    // The same unmount, for the other held state. The text is the last copy
    // of a file the person deleted, so losing it here loses the note.
    const EditorInstance = (await import("../EditorInstance")).default;
    const { keepUnsavedForRecovery } = await import("../../../services/autosave");
    const { recordUnsavedNotes } = await import("../../../services/tauri");

    bufferContent.set("A", "as Writ opened it");
    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [active, setActive] = createSignal<BufferDocument | null>(mockBuffer("A"));
    const { container } = render(() => (
      <WindowProvider windowId={9402}>
        <Probe />
        <Show when={active()}>{(buffer) => <EditorInstance buffer={buffer()} />}</Show>
      </WindowProvider>
    ));
    await flush();

    const view = EditorView.findFromDOM(
      container.querySelector(".cm-editor") as HTMLElement,
    );
    view!.dispatch({
      changes: { from: view!.state.doc.length, insert: " typed after it went" },
    });
    await flush();

    win!.editor.recordFileEvent("A", "removed");
    await flush();

    setActive(null);
    await flush();

    win!.editor.noteClosed("A");
    await keepUnsavedForRecovery("A");

    expect(vi.mocked(recordUnsavedNotes)).toHaveBeenCalledWith([
      { id: "A", content: "as Writ opened it typed after it went" },
    ]);
  });

  it("still cancels the autosave of a note nothing is holding", async () => {
    // The cleanup's own job. A note that may write has its queue and its
    // timers dropped with the view, and nothing is handed over for it.
    const EditorInstance = (await import("../EditorInstance")).default;
    const { keepUnsavedForRecovery, peekUnsavedContent } = await import(
      "../../../services/autosave"
    );
    const { recordUnsavedNotes } = await import("../../../services/tauri");

    bufferContent.set("A", "as Writ opened it");
    let win: ReturnType<typeof useWindow> | null = null;
    function Probe() {
      win = useWindow();
      return null;
    }

    const [active, setActive] = createSignal<BufferDocument | null>(mockBuffer("A"));
    render(() => (
      <WindowProvider windowId={9403}>
        <Probe />
        <Show when={active()}>{(buffer) => <EditorInstance buffer={buffer()} />}</Show>
      </WindowProvider>
    ));
    await flush();

    setActive(null);
    await flush();

    expect(peekUnsavedContent("A")).toBeUndefined();

    win!.editor.noteClosed("A");
    await keepUnsavedForRecovery("A");
    expect(vi.mocked(recordUnsavedNotes)).not.toHaveBeenCalled();
  });
});
