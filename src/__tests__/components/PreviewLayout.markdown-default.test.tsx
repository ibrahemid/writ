import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { configStore } from "../../stores/global/config";
import type { BufferDocument } from "../../types/buffer";
import type { LayoutMode } from "../../lib/preview-layout";

// A markdown file is laid out before the editor mounts. The persisted read is
// async, so the layout the store answers while it is in flight is the one the
// editor is built with; the persisted value replaces it when it lands.

const mocks = vi.hoisted(() => ({
  previewGetLayout: vi.fn(),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  previewRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: mocks.previewGetLayout,
  previewSetLayout: mocks.previewSetLayout,
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

// The layout decision is what this pins, not CodeMirror.
vi.mock("../../components/Editor/EditorInstance", async () => {
  const { createEffect } = await import("solid-js");
  const { useWindow } = await import("../../components/WindowProvider/WindowProvider");
  return {
    default: (props: { buffer: { id: string } }) => {
      const win = useWindow();
      createEffect(() => win.editor.setCurrentBufferId(props.buffer.id));
      return <div data-testid="editor-stub" />;
    },
  };
});

import PreviewLayout from "../../components/Preview/PreviewLayout";

function markdownBuffer(): BufferDocument {
  return {
    id: "M1",
    title: "plan.md",
    filename: "plan.md",
    status: "active",
    language: null,
    source_path: "/files/plan.md",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

function readLayout(): LayoutMode | null {
  let held: LayoutMode | null = null;
  function Probe() {
    held = useWindow().layout.get("M1", "markdown");
    return null;
  }
  render(() => (
    <WindowProvider windowId={9601}>
      <Probe />
      <PreviewLayout buffer={markdownBuffer()} />
    </WindowProvider>
  ));
  return held;
}

describe("PreviewLayout: a markdown buffer is laid out before the editor mounts", () => {
  beforeEach(() => {
    rendererRegistry.setFromIpc([
      {
        content_type: "markdown",
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
    vi.restoreAllMocks();
    mocks.previewGetLayout.mockReset();
  });

  it("reads as inline while the persisted layout is still in flight", () => {
    mocks.previewGetLayout.mockReturnValue(new Promise(() => {}));

    expect(readLayout()).toEqual({ kind: "inline" });
  });

  it("takes the persisted layout once it lands", async () => {
    let land: ((value: { layout: string; ratio: number | null }) => void) | null = null;
    mocks.previewGetLayout.mockReturnValue(
      new Promise<{ layout: string; ratio: number | null }>((resolve) => {
        land = resolve;
      }),
    );

    expect(readLayout()).toEqual({ kind: "inline" });

    land!({ layout: "source", ratio: null });

    const win = (await import("../../stores/global/window-registry")).windowRegistry.getActive();
    await waitFor(() => expect(win!.layout.get("M1", "markdown")).toEqual({ kind: "source" }));
  });

  it("reads as source while in flight where the default layout is source", () => {
    const held = configStore.config();
    vi.spyOn(configStore, "config").mockReturnValue({
      ...held,
      preview: { ...held.preview, default_layout_markdown: "source" },
    });
    mocks.previewGetLayout.mockReturnValue(new Promise(() => {}));

    expect(readLayout()).toEqual({ kind: "source" });
  });
});
