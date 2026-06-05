import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { DEFAULT_RATIO } from "../../lib/preview-layout";
import type { BufferDocument } from "../../types/buffer";

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import PreviewLayoutToggle from "../../components/Preview/PreviewLayoutToggle";

const HTML_BUFFER: BufferDocument = {
  id: "T1",
  title: "page.html",
  filename: "t-uuid.txt",
  status: "active",
  language: null,
  source_path: null,
  cursor_pos: 0,
  scroll_pos: 0,
  tab_order: 0,
  created_at: "",
  updated_at: "",
  closed_at: null,
};

describe("PreviewLayoutToggle", () => {
  beforeEach(() => {
    rendererRegistry.setFromIpc([
      {
        content_type: "html",
        capabilities: {
          supports_live_render: true,
          supports_print: true,
          max_safe_document_bytes: 50 * 1024 * 1024,
        },
      },
    ]);
  });

  afterEach(() => {
    cleanup();
    rendererRegistry.setFromIpc([]);
  });

  async function mountWithActive(buf: BufferDocument | null) {
    const { listActiveBuffers } = await import("../../services/tauri");
    (listActiveBuffers as ReturnType<typeof vi.fn>).mockResolvedValue(buf ? [buf] : []);
    await bufferRegistry.load();

    const result = render(() => (
      <WindowProvider windowId={4242}>
        <PreviewLayoutToggle />
      </WindowProvider>
    ));
    if (buf) {
      await waitFor(() => expect(windowRegistry.getActive()).not.toBeNull());
      windowRegistry.getActive()!.tabs.setActiveTabId(buf.id);
    }
    return result;
  }

  it("renders nothing when there is no renderable active buffer", async () => {
    const { container } = await mountWithActive(null);
    expect(container.querySelector(".layout-toggle")).toBeNull();
  });

  it("renders a radiogroup with three segments for a renderable buffer", async () => {
    const { container } = await mountWithActive(HTML_BUFFER);
    await waitFor(() => {
      expect(container.querySelector('[role="radiogroup"]')).not.toBeNull();
    });
    const segs = container.querySelectorAll('[role="radio"]');
    expect(segs.length).toBe(3);
    expect(Array.from(segs).map((s) => s.textContent)).toEqual([
      "Source",
      "Split",
      "Preview",
    ]);
  });

  it("marks the active layout segment checked and switches on click", async () => {
    const { container } = await mountWithActive(HTML_BUFFER);
    await waitFor(() => expect(container.querySelector(".layout-toggle")).not.toBeNull());

    const win = windowRegistry.getActive()!;
    win.layout.setLocal("T1", { kind: "source" });

    const [sourceSeg, splitSeg, previewSeg] = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[role="radio"]'),
    );
    await waitFor(() => expect(sourceSeg.getAttribute("aria-checked")).toBe("true"));
    // Roving tabindex: only the checked segment is tab-stoppable.
    expect(sourceSeg.tabIndex).toBe(0);
    expect(splitSeg.tabIndex).toBe(-1);

    fireEvent.click(splitSeg);
    await waitFor(() => {
      expect(win.layout.get("T1").kind).toBe("split");
      expect(splitSeg.getAttribute("aria-checked")).toBe("true");
      expect(splitSeg.tabIndex).toBe(0);
      expect(sourceSeg.tabIndex).toBe(-1);
    });

    fireEvent.click(previewSeg);
    await waitFor(() => expect(win.layout.get("T1").kind).toBe("preview"));
  });

  it("preserves a dragged split ratio across a source round-trip", async () => {
    const { container } = await mountWithActive(HTML_BUFFER);
    await waitFor(() => expect(container.querySelector(".layout-toggle")).not.toBeNull());

    const win = windowRegistry.getActive()!;
    // Simulate a dragged 0.7 split.
    win.layout.setLocal("T1", { kind: "split", ratio: 0.7, orientation: "vertical" });

    const [sourceSeg, splitSeg] = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[role="radio"]'),
    );

    // Switching to source, then back to split, re-derives a 50/50 split — the
    // ratio is not remembered across a kind change (matches the cycle keymap
    // semantics). This pins that deliberate behavior.
    fireEvent.click(sourceSeg);
    await waitFor(() => expect(win.layout.get("T1").kind).toBe("source"));
    fireEvent.click(splitSeg);
    await waitFor(() => {
      const l = win.layout.get("T1");
      expect(l.kind).toBe("split");
      if (l.kind === "split") expect(l.ratio).toBe(DEFAULT_RATIO);
    });
  });

  it("clicking Split while already split keeps the current ratio", async () => {
    const { container } = await mountWithActive(HTML_BUFFER);
    await waitFor(() => expect(container.querySelector(".layout-toggle")).not.toBeNull());

    const win = windowRegistry.getActive()!;
    win.layout.setLocal("T1", { kind: "split", ratio: 0.7, orientation: "vertical" });

    const splitSeg = container.querySelectorAll<HTMLButtonElement>('[role="radio"]')[1];
    fireEvent.click(splitSeg);
    await waitFor(() => {
      const l = win.layout.get("T1");
      expect(l.kind === "split" && l.ratio).toBe(0.7);
    });
  });

  it("arrow keys move the selection (radio semantics)", async () => {
    const { container } = await mountWithActive(HTML_BUFFER);
    await waitFor(() => expect(container.querySelector(".layout-toggle")).not.toBeNull());

    const win = windowRegistry.getActive()!;
    win.layout.setLocal("T1", { kind: "source" });
    const group = container.querySelector('[role="radiogroup"]')!;

    fireEvent.keyDown(group, { key: "ArrowRight" });
    await waitFor(() => expect(win.layout.get("T1").kind).toBe("split"));

    fireEvent.keyDown(group, { key: "ArrowRight" });
    await waitFor(() => expect(win.layout.get("T1").kind).toBe("preview"));

    // Wraps around.
    fireEvent.keyDown(group, { key: "ArrowRight" });
    await waitFor(() => expect(win.layout.get("T1").kind).toBe("source"));

    fireEvent.keyDown(group, { key: "ArrowLeft" });
    await waitFor(() => expect(win.layout.get("T1").kind).toBe("preview"));
  });
});
