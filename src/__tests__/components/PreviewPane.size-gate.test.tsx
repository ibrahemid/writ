import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import type { WindowState } from "../../stores/window/createWindowState";
import type { BufferDocument } from "../../types/buffer";

const mocks = vi.hoisted(() => ({
  render: vi.fn().mockResolvedValue({ kind: "rendered" as const, used_fallback_stylesheet: true, parser_warnings: [] }),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.render,
  previewRender: mocks.render,
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import PreviewPane from "../../components/Preview/PreviewPane";
import { configStore } from "../../stores/global/config";
import { showInEditor } from "../fixtures/editor-view";

function page(id: string): BufferDocument {
  return {
    id,
    title: `${id}.html`,
    filename: `${id}.html`,
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
}

function load(win: WindowState, id: string, text: string): void {
  showInEditor(win, id, text);
}

async function refuseAbove(bytes: number): Promise<void> {
  const current = configStore.config();
  await configStore.save({
    ...current,
    preview: {
      ...current.preview,
      live_render_threshold_mb: bytes / (1024 * 1024),
      render_refuse_threshold_mb: bytes / (1024 * 1024),
    },
  });
}

function mount(windowId: number, id: string): WindowState {
  render(() => (
    <WindowProvider windowId={windowId}>
      <PreviewPane buffer={page(id)} contentType="html" isActive={true} />
    </WindowProvider>
  ));
  const win = windowRegistry.getActive();
  if (!win) throw new Error("no window");
  return win;
}

describe("PreviewPane size gate", () => {
  beforeEach(() => {
    mocks.render.mockClear();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("measures the document without encoding it", async () => {
    await refuseAbove(50 * 1024 * 1024);
    const encode = vi.spyOn(TextEncoder.prototype, "encode");
    const text = `<body>${"word ".repeat(60_000)}</body>`;
    const win = mount(9321, "P1");
    load(win, "P1", text);

    await waitFor(() => expect(mocks.render).toHaveBeenCalled());
    const whole = encode.mock.calls.filter(([input]) => (input?.length ?? 0) >= text.length);
    expect(whole).toEqual([]);
  });

  it("counts a character outside ASCII as the bytes UTF-8 gives it", async () => {
    await refuseAbove(1000);
    const win = mount(9322, "P2");
    load(win, "P2", "é".repeat(600));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(mocks.render).not.toHaveBeenCalled();

    cleanup();
    const next = mount(9323, "P3");
    load(next, "P3", "é".repeat(400));
    await waitFor(() => expect(mocks.render).toHaveBeenCalled());
  });

  it("counts a character outside the Basic Multilingual Plane as four bytes", async () => {
    await refuseAbove(1000);
    const win = mount(9324, "P4");
    load(win, "P4", "😀".repeat(260));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(mocks.render).not.toHaveBeenCalled();
  });
});
