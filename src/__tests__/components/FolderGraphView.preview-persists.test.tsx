import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import type { BufferDocument } from "../../types/buffer";

// The folder graph is a layer over the note, and the preview underneath keeps
// its element: taking a loaded writ-preview:// iframe out of the page freezes
// the macOS webview outright (PR #127). The layer hides the pane with `hidden`
// and never removes it, and this is what holds that.

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.forceRender,
  previewRender: mocks.forceRender,
  previewClose: mocks.previewClose,
  previewGetLayout: mocks.previewGetLayout,
  previewSetLayout: mocks.previewSetLayout,
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../components/Editor/EditorInstance", async () => {
  const { createEffect, onCleanup } = await import("solid-js");
  const { useWindow } = await import("../../components/WindowProvider/WindowProvider");
  return {
    default: (props: { buffer: { id: string } }) => {
      const win = useWindow();
      createEffect(() => win.editor.setCurrentBufferId(props.buffer.id));
      onCleanup(() => win.editor.setCurrentBufferId(null));
      return <div data-testid="editor-stub" />;
    },
  };
});

import PreviewLayout from "../../components/Preview/PreviewLayout";

function htmlBuffer(): BufferDocument {
  return {
    id: "H1",
    title: "page.html",
    filename: "page.html",
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
  };
}

function frame(container: HTMLElement): HTMLIFrameElement | null {
  return container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
}

describe("the preview while the folder graph is open", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
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

  it("is the same element it was, hidden rather than taken out", async () => {
    const { container } = render(() => (
      <WindowProvider windowId={7401}>
        <PreviewLayout buffer={htmlBuffer()} />
      </WindowProvider>
    ));

    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/));
    const original = frame(container);
    expect(original).not.toBeNull();

    const win = windowRegistry.getActive();
    win!.folderGraph.open();

    await waitFor(() =>
      expect(container.querySelector(".preview-pane-slot")?.hasAttribute("hidden")).toBe(true),
    );
    expect(frame(container)).toBe(original);
    expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/);

    win!.folderGraph.close();
    await waitFor(() =>
      expect(container.querySelector(".preview-pane-slot")?.hasAttribute("hidden")).toBe(false),
    );
    expect(frame(container)).toBe(original);
  });
});
