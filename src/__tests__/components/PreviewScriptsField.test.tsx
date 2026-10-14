import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// The status bar field for a previewed document's own scripts: it reads the
// stored opt-in, a click runs the toggle command, and it follows the backend
// when another program's change lets the opt-in lapse, whether the change
// arrives as an event or a render finds it.

const mocks = vi.hoisted(() => ({
  getScripts: vi.fn(),
  setScripts: vi.fn(),
  toast: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  previewGetScripts: mocks.getScripts,
  previewSetScripts: mocks.setScripts,
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: mocks.toast,
}));

import WindowProvider from "../../components/WindowProvider/WindowProvider";
import PreviewScriptsField from "../../components/Preview/PreviewScriptsField";
import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { previewScriptsStore } from "../../stores/global/preview-scripts";
import { unregisterCommand } from "../../commands/registry";
import { registerPreviewKeymap } from "../../keymap/preview";
import { defaultSplit, type LayoutMode } from "../../lib/preview-layout";
import { createExternalEditDeps } from "../../lib/external-edit-deps";
import { handleExternalEdit } from "../../services/external-edit";
import type { BufferDocument } from "../../types/buffer";

const PREVIEW_COMMANDS = [
  "preview.cycleLayout",
  "preview.refresh",
  "preview.toggleFullscreen",
  "preview.exitFullscreen",
  "preview.swapOrientation",
  "preview.resetRatio",
  "preview.toggleRunScripts",
];

const CAPABILITIES = {
  supports_live_render: true,
  supports_print: true,
  max_safe_document_bytes: 50 * 1024 * 1024,
};

const HTML_FILE: BufferDocument = {
  id: "S1",
  title: "report.html",
  filename: "s1.txt",
  status: "active",
  language: null,
  source_path: null,
  cursor_pos: 0,
  scroll_pos: 0,
  tab_order: 0,
  created_at: "",
  updated_at: "",
  closed_at: null,
  read_only: false,
  size_bytes: 0,
  line_ending: "lf",
  encoding: "UTF-8",
};

let nextWindowId = 7100;

async function mountWith(buf: BufferDocument, layout: LayoutMode) {
  const { listActiveBuffers } = await import("../../services/tauri");
  (listActiveBuffers as ReturnType<typeof vi.fn>).mockResolvedValue([buf]);
  await bufferRegistry.load();

  nextWindowId += 1;
  const result = render(() => (
    <WindowProvider windowId={nextWindowId}>
      <PreviewScriptsField />
    </WindowProvider>
  ));
  await waitFor(() => expect(windowRegistry.getActive()).not.toBeNull());
  const win = windowRegistry.getActive()!;
  win.layout.setLocal(buf.id, layout);
  win.tabs.setActiveTabId(buf.id);
  return result;
}

describe("PreviewScriptsField", () => {
  beforeEach(() => {
    rendererRegistry.setFromIpc([
      { content_type: "html", capabilities: CAPABILITIES },
      { content_type: "markdown", capabilities: CAPABILITIES },
      { content_type: "mermaid", capabilities: CAPABILITIES },
    ]);
    registerPreviewKeymap();
    mocks.getScripts.mockReset();
    mocks.setScripts.mockReset().mockResolvedValue(undefined);
    mocks.toast.mockReset();
  });

  afterEach(() => {
    cleanup();
    for (const id of PREVIEW_COMMANDS) unregisterCommand(id);
    rendererRegistry.setFromIpc([]);
  });

  it("reads Scripts off for a previewed HTML file that is not opted in", async () => {
    mocks.getScripts.mockResolvedValue(false);
    const { findByRole } = await mountWith({ ...HTML_FILE, id: "S1" }, defaultSplit());

    const field = await findByRole("button", { name: "Scripts off" });
    expect(mocks.getScripts).toHaveBeenCalledWith("S1");
    expect(field.classList.contains("is-on")).toBe(false);
    expect(field.getAttribute("title")).toBe("Toggle scripts for this file");
  });

  it("reads Scripts on for a file that is opted in", async () => {
    mocks.getScripts.mockResolvedValue(true);
    const { findByRole } = await mountWith({ ...HTML_FILE, id: "S2" }, { kind: "preview" });

    const field = await findByRole("button", { name: "Scripts on" });
    expect(field.classList.contains("is-on")).toBe(true);
  });

  it("shows nothing when the preview is not showing a document with scripts", async () => {
    mocks.getScripts.mockResolvedValue(true);
    for (const [buf, layout] of [
      [{ ...HTML_FILE, id: "S3" }, { kind: "source" }],
      [{ ...HTML_FILE, id: "S4", title: "flow.mmd" }, defaultSplit()],
      [{ ...HTML_FILE, id: "S5", title: "notes.txt" }, defaultSplit()],
    ] as const) {
      const { container } = await mountWith(buf, layout);
      await Promise.resolve();
      expect(container.querySelector(".statusbar-scripts")).toBeNull();
      cleanup();
    }
    expect(mocks.getScripts).not.toHaveBeenCalled();
  });

  it("runs the toggle command on a click and follows the stored choice", async () => {
    mocks.getScripts.mockResolvedValue(false);
    const { findByRole } = await mountWith({ ...HTML_FILE, id: "S6" }, defaultSplit());
    const win = windowRegistry.getActive()!;
    const before = win.preview.forceRefreshToken();

    fireEvent.click(await findByRole("button", { name: "Scripts off" }));

    await findByRole("button", { name: "Scripts on" });
    expect(mocks.setScripts).toHaveBeenCalledWith("S6", true);
    expect(win.preview.forceRefreshToken()).toBe(before + 1);
    expect(mocks.toast).toHaveBeenCalledWith("Scripts on for this file", "info");
  });

  it("reads Scripts off once the backend lets the opt-in lapse with a change", async () => {
    mocks.getScripts.mockResolvedValue(true);
    const { findByRole } = await mountWith({ ...HTML_FILE, id: "S7" }, defaultSplit());
    await findByRole("button", { name: "Scripts on" });
    mocks.getScripts.mockResolvedValue(false);

    previewScriptsStore.refresh("S7");

    await findByRole("button", { name: "Scripts off" });
    expect(mocks.setScripts).not.toHaveBeenCalled();
  });

  it("reads Scripts off when the file changes on disk, through the app's own wiring", async () => {
    mocks.getScripts.mockResolvedValue(true);
    const file = { ...HTML_FILE, id: "S9" };
    const { findByRole } = await mountWith(file, defaultSplit());
    await findByRole("button", { name: "Scripts on" });
    const win = windowRegistry.getActive()!;
    const before = win.preview.forceRefreshToken();
    const reload = vi.fn();
    const deps = createExternalEditDeps({
      editor: {
        isDirty: () => false,
        isRemovedOnDisk: () => false,
        requestExternalReload: reload,
        recordFileEvent: () => {},
      },
      openBuffers: () => [file],
      refreshBuffer: () => Promise.resolve(),
      forgetSaveStatus: () => {},
      refreshScripts: (id) => previewScriptsStore.refresh(id),
    });
    mocks.getScripts.mockResolvedValue(false);

    await handleExternalEdit({ bufferId: "S9", change: "modified" }, deps);

    await findByRole("button", { name: "Scripts off" });
    expect(win.preview.forceRefreshToken()).toBe(before + 1);
    expect(reload).toHaveBeenCalledWith("S9");
    expect(mocks.setScripts).not.toHaveBeenCalled();
  });

  it("reads Scripts off once a render finds the opt-in lapsed", async () => {
    mocks.getScripts.mockResolvedValue(true);
    const { findByRole } = await mountWith({ ...HTML_FILE, id: "S8" }, defaultSplit());
    await findByRole("button", { name: "Scripts on" });

    previewScriptsStore.settle("S8", previewScriptsStore.issue(), false);

    await findByRole("button", { name: "Scripts off" });
  });
});
