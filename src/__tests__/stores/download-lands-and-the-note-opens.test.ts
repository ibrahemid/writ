import { describe, it, expect, vi, afterEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// The file's digest comes from Rust, and these cases are about the window in
// which that answer has not come back yet, so the promise is resolved by hand.
const backend = vi.hoisted(() => ({
  pending: [] as Array<(answer: unknown) => void>,
}));

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(
    () =>
      new Promise((resolve) => {
        backend.pending.push(resolve);
      }),
  ),
  restoreNoteFile: vi.fn(),
  materialiseNote: vi.fn().mockResolvedValue(undefined),
  cancelMaterialiseNote: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async () => () => undefined),
}));

import { createEditorStore } from "../../stores/window/editor-store";
import { createDownloadStore } from "../../stores/window/download-store";
import { createTabStore } from "../../stores/window/tab-store";
import { hashDocument } from "../../lib/doc-hash";
import { resetAutosave } from "../../services/autosave";

const PATH = "/notes/away.md";
const TEXT = "what the bytes carried\n";

/** Lets the store's awaits run without answering any of them. */
async function turns(count = 12): Promise<void> {
  for (let i = 0; i < count; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
}

function described(hash: string) {
  return { state: "described", disk: { hash, size: 1, mtime_ms: null } };
}

const doc = { id: "away", title: "away", source_path: PATH } as unknown as BufferDocument;

/**
 * The window at the moment the bytes land: a note tab already open, the
 * download running behind it, and the tab store wired into the download store
 * the way `createWindowState` wires them.
 */
function windowWithADownloadRunning() {
  const editor = createEditorStore();
  const downloads = createDownloadStore();
  const openFile = vi.fn(async () => ({
    kind: "opened" as const,
    doc,
    existed: false,
    mode: { kind: "Normal" as const },
  }));
  const registry = {
    openFile,
    activeTabs: () => [] as BufferDocument[],
  } as unknown as Parameters<typeof createTabStore>[0]["registry"];
  const tabs = createTabStore({ registry, downloads, editor });
  downloads.attachOpener(tabs.openFile);
  return { editor, downloads, tabs, openFile };
}

afterEach(() => {
  backend.pending.length = 0;
  resetAutosave();
  vi.clearAllMocks();
});

describe("a note whose download landed behind another one", () => {
  it("opens through the ordinary path, without taking the screen", async () => {
    // "done" for a download the person is not watching reopens with
    // `activate: false`, which is the same `openFile` a note opened from
    // anywhere else takes. Nothing about the record it leaves says download.
    const { editor, downloads, tabs, openFile } = windowWithADownloadRunning();
    await downloads.start({ path: PATH, title: "away.md", provider: "iCloud Drive" });
    downloads.select(null);

    await downloads.handle({ path: PATH, state: "done" });

    expect(openFile).toHaveBeenCalledWith(PATH);
    expect(downloads.pending()).toEqual([]);
    expect(tabs.activeTabId()).toBeNull();

    editor.stopSaveListener();
  });

  it("keeps what a write put on the file while its open was still out", async () => {
    // The open is driven the way `EditorInstance.tsx:528` drives it, which is
    // the one call that stamps the open's ticket. A write landing before the
    // read comes back is the later word on the file, so the read's digest is
    // the one that loses.
    const { editor, downloads } = windowWithADownloadRunning();
    await downloads.start({ path: PATH, title: "away.md", provider: "iCloud Drive" });
    downloads.select(null);
    await downloads.handle({ path: PATH, state: "done" });

    const written = await hashDocument(TEXT);
    editor.noteOpened(doc.id, TEXT);
    await turns();

    expect(editor.isTracked(doc.id)).toBe(false);

    editor.noteSaved(doc.id, written, false);
    await turns();

    backend.pending[0]?.(described("read before the write landed"));
    await turns();

    expect(editor.lastKnownDiskHash(doc.id)).toBe(written);
    expect(editor.isDirty(doc.id)).toBe(false);
    expect(editor.isTracked(doc.id)).toBe(true);

    editor.stopSaveListener();
  });
});
