import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { defaultSplit, type LayoutMode } from "../../lib/preview-layout";
import type { BufferDocument } from "../../types/buffer";

// Pins the invariant that survives the layout modes: div.preview-pane-slot is
// outside every Show, so the writ-preview:// iframe is navigated and never
// removed (removing a loaded one freezes the macOS webview, #124).

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

function mdBuffer(): BufferDocument {
  return {
    id: "M1",
    title: "plan.md",
    filename: "plan.md",
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

const MODES: LayoutMode[] = [
  { kind: "source" },
  { kind: "inline" },
  defaultSplit(),
  { kind: "preview" },
];

describe("PreviewLayout — the pane slot stays mounted in every layout mode", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
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
  });

  it("keeps the preview pane slot mounted in every layout mode", async () => {
    const { container } = render(() => (
      <WindowProvider windowId={7401}>
        <PreviewLayout buffer={mdBuffer()} />
      </WindowProvider>
    ));

    await waitFor(() => expect(frame(container)).not.toBeNull());
    const original = frame(container);
    const win = windowRegistry.getActive()!;

    for (const mode of MODES) {
      win.layout.setLocal("M1", mode);
      await waitFor(() =>
        expect(container.querySelector(".preview-pane-slot")).not.toBeNull(),
      );
      expect(frame(container)).toBe(original);
    }
  });

  it("keeps the preview pane slot mounted when no buffer is open", async () => {
    const [buf, setBuf] = createSignal<BufferDocument | null>(mdBuffer());
    const { container } = render(() => (
      <WindowProvider windowId={7402}>
        <PreviewLayout buffer={buf()} />
      </WindowProvider>
    ));

    await waitFor(() => expect(frame(container)).not.toBeNull());
    const original = frame(container);

    setBuf(null);
    await waitFor(() =>
      expect(container.querySelector(".editor-empty")).not.toBeNull(),
    );
    expect(frame(container)).toBe(original);
  });
});
