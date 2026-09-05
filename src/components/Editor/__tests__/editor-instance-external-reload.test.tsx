import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { WindowState } from "../../../stores/window/createWindowState";

// The file, as the operating system holds it. `readBufferContent` is the Rust
// command that reads it, so this map is what "on disk" means here.
const fileOnDisk = new Map<string, string>();

const readBufferContent = vi.fn(async (id: string) => fileOnDisk.get(id) ?? "");

vi.mock("../../../services/tauri", () => ({
  readBufferContent: (id: string) => readBufferContent(id),
  saveBufferContent: vi.fn(async () => null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
}));

// The registry's read is a straight pass-through to the Rust command. Its
// `load` is deliberately absent: a reload path that called it would recreate
// the always-mounted preview iframe and hard-freeze the macOS webview
// (PR #127), so a call to it here fails the test rather than freezing it.
vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: (id: string) => readBufferContent(id),
  },
}));

function mockBuffer(id: string): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.md`,
    status: "active",
    language: null,
    source_path: `/somewhere/else/${id}.md`,
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

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  let win!: WindowState;
  const Probe = () => {
    win = useWindow();
    return null;
  };
  const result = render(() => (
    <WindowProvider windowId={9401}>
      <Probe />
      <EditorInstance buffer={buffer} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, win };
}

describe("EditorInstance: reloading after a change outside Writ", () => {
  beforeEach(() => {
    fileOnDisk.clear();
    readBufferContent.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it("shows what the file holds now, not what Writ was holding", async () => {
    // The whole point of the watcher: another program rewrote the file, and
    // the editor has to end up showing those bytes. Asserting the text in the
    // view rather than a call count, because a reload that read a copy Writ
    // was keeping would make the same calls and show the wrong thing.
    fileOnDisk.set("note", "as Writ opened it\n");
    const { container, win } = await mount(mockBuffer("note"));
    expect(container.textContent).toContain("as Writ opened it");

    fileOnDisk.set("note", "rewritten by another program\n");
    win.editor.requestExternalReload("note");

    await waitFor(() =>
      expect(container.textContent).toContain("rewritten by another program"),
    );
    expect(container.textContent).not.toContain("as Writ opened it");
  });

  it("goes back to the file for every change, not once", async () => {
    fileOnDisk.set("note", "first\n");
    const { container, win } = await mount(mockBuffer("note"));

    fileOnDisk.set("note", "second\n");
    win.editor.requestExternalReload("note");
    await waitFor(() => expect(container.textContent).toContain("second"));

    fileOnDisk.set("note", "third\n");
    win.editor.requestExternalReload("note");
    await waitFor(() => expect(container.textContent).toContain("third"));
  });
});
