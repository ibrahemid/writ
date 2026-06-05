import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import type { BufferDocument } from "../../types/buffer";

// A buffer whose content type is RECOGNIZED (contentTypeForBuffer returns a
// non-null id) but has NO registered renderer must never mount a blank
// iframe. It falls to source; if the user explicitly cycles to a preview
// layout via the keymap, a friendly "no preview" note shows instead of an
// empty pane. This guards the class of bug the L2 smoke surfaced for .md
// (recognized, unregistered) and any future recognized-but-unbuilt type.

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "no_renderer" as const,
    content_type: "specialtype",
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

// Recognize a synthetic content type for the test buffer so it's
// "recognized but unregistered" — the registry below has no renderer for it.
vi.mock("../../lib/content-type", () => ({
  contentTypeForBuffer: () => "specialtype",
}));

// Registry mock: NO renderer for "specialtype" (or anything).
vi.mock("../../stores/global/renderer-registry", () => ({
  rendererRegistry: {
    hasRenderer: (ct: string | null) => ct !== null && ct === "html",
    get: () => null,
    setFromIpc: vi.fn(),
    renderers: () => [],
  },
}));

vi.mock("../../components/Editor/EditorInstance", () => ({
  default: () => <div data-testid="editor-stub" />,
}));

import PreviewLayout from "../../components/Preview/PreviewLayout";
import { defaultSplit } from "../../lib/preview-layout";

function recognizedUnregisteredBuffer(): BufferDocument {
  return {
    id: "U1",
    title: "thing.specialtype",
    filename: "u-uuid.txt",
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

describe("PreviewLayout — recognized but unregistered content type", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
  });

  afterEach(() => cleanup());

  it("resolves to source layout and never mounts an iframe", async () => {
    const { container } = render(() => (
      <WindowProvider windowId={7001}>
        <PreviewLayout buffer={recognizedUnregisteredBuffer()} />
      </WindowProvider>
    ));

    // Let the async initLayout effect settle.
    await waitFor(() => {
      // Editor is present (we never hid it for an unrenderable buffer).
      expect(container.querySelector('[data-testid="editor-stub"]')).not.toBeNull();
    });

    // The hard guarantee: no preview iframe is ever in the DOM.
    expect(container.querySelector("iframe.preview-frame")).toBeNull();
    // And the renderer IPC was never invoked for an unrenderable buffer.
    expect(mocks.forceRender).not.toHaveBeenCalled();
  });

  it("shows a friendly note (not a blank iframe) if cycled into a preview layout", async () => {
    render(() => (
      <WindowProvider windowId={7002}>
        <PreviewLayout buffer={recognizedUnregisteredBuffer()} />
      </WindowProvider>
    ));

    // Simulate the keymap cycling this buffer into split, which sets the
    // layout without a renderer check.
    await waitFor(() => expect(windowRegistry.getActive()).not.toBeNull());
    const win = windowRegistry.getActive()!;
    win.layout.setLocal("U1", defaultSplit());

    // Still no iframe — the unsupported note takes the pane slot.
    await waitFor(() => {
      expect(document.querySelector(".preview-unsupported")).not.toBeNull();
    });
    expect(document.querySelector("iframe.preview-frame")).toBeNull();
    expect(mocks.forceRender).not.toHaveBeenCalled();
  });
});
