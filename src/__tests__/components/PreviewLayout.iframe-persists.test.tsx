import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import type { BufferDocument } from "../../types/buffer";

// Regression for the preview-close webview freeze. Tearing down a loaded
// writ-preview:// iframe element hard-freezes the macOS webview (PR #124 fixed
// the non-last-tab case by reselecting; the last-tab `active->null` and the
// switch-to-non-renderable cases still removed the element). The fix keeps the
// preview pane mounted always and only NAVIGATES its iframe (to a blank doc
// when no preview shows). This test pins the invariant at the DOM level: the
// SAME iframe element survives going to no-buffer and to a non-renderable
// buffer — it is navigated, never removed.

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

// Stub EditorInstance but honor the editor contract the preview depends on:
// publish the loaded buffer id (and clear it when no buffer is mounted).
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
  };
}

function txtBuffer(): BufferDocument {
  return { ...htmlBuffer(), id: "T1", title: "notes.txt", filename: "notes.txt" };
}

function frame(container: HTMLElement): HTMLIFrameElement | null {
  return container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
}

describe("PreviewLayout — preview iframe is never torn down (preview-close freeze)", () => {
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

  it("keeps the same iframe element when the buffer goes to none and to non-renderable", async () => {
    const [buf, setBuf] = createSignal<BufferDocument | null>(htmlBuffer());

    const { container } = render(() => (
      <WindowProvider windowId={7301}>
        <PreviewLayout buffer={buf()} />
      </WindowProvider>
    ));

    // Renderable html in split: the iframe shows a live document render.
    await waitFor(() =>
      expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/),
    );
    const original = frame(container);
    expect(original).not.toBeNull();

    // Last tab closes → no active buffer. The element must survive (navigated
    // to the blank doc), not be removed.
    setBuf(null);
    await waitFor(() => expect(frame(container)!.src).toMatch(/chrome\/blank$/));
    expect(frame(container)).toBe(original);
    expect(container.querySelector('.preview-pane-slot.is-hidden')).not.toBeNull();

    // Switch to a non-renderable buffer → still the same element, still parked.
    setBuf(txtBuffer());
    const win = windowRegistry.getActive();
    win!.editor.setCurrentBufferId("T1");
    await waitFor(() =>
      expect(container.querySelector('[data-testid="editor-stub"]')).not.toBeNull(),
    );
    expect(frame(container)).toBe(original);
    expect(frame(container)!.src).toMatch(/chrome\/blank$/);
  });
});
