import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useActiveBuffer } from "../../lib/use-active-buffer";
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

function makeBuffer(id: string): BufferDocument {
  return {
    id,
    title: `${id}.txt`,
    filename: `${id}-uuid.txt`,
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
  };
}

function Probe() {
  const active = useActiveBuffer();
  return <div data-testid="active">{active()?.id ?? "none"}</div>;
}

async function mount(buffers: BufferDocument[]) {
  const { listActiveBuffers } = await import("../../services/tauri");
  (listActiveBuffers as ReturnType<typeof vi.fn>).mockResolvedValue(buffers);
  await bufferRegistry.load();
  const result = render(() => (
    <WindowProvider windowId={7777}>
      <Probe />
    </WindowProvider>
  ));
  await waitFor(() => expect(windowRegistry.getActive()).not.toBeNull());
  return result;
}

describe("useActiveBuffer", () => {
  afterEach(() => {
    cleanup();
  });

  it("is null when there is no active tab", async () => {
    const { getByTestId } = await mount([makeBuffer("A"), makeBuffer("B")]);
    expect(getByTestId("active").textContent).toBe("none");
  });

  it("resolves the loaded buffer matching the active tab id", async () => {
    const { getByTestId } = await mount([makeBuffer("A"), makeBuffer("B")]);
    windowRegistry.getActive()!.tabs.setActiveTabId("B");
    await waitFor(() => expect(getByTestId("active").textContent).toBe("B"));
  });

  it("is null when the active tab id has no matching buffer", async () => {
    const { getByTestId } = await mount([makeBuffer("A")]);
    windowRegistry.getActive()!.tabs.setActiveTabId("gone");
    await waitFor(() => expect(getByTestId("active").textContent).toBe("none"));
  });

  it("tracks the active buffer reactively across a tab switch", async () => {
    const { getByTestId } = await mount([makeBuffer("A"), makeBuffer("B")]);
    const tabs = windowRegistry.getActive()!.tabs;
    tabs.setActiveTabId("A");
    await waitFor(() => expect(getByTestId("active").textContent).toBe("A"));
    tabs.setActiveTabId("B");
    await waitFor(() => expect(getByTestId("active").textContent).toBe("B"));
  });
});
