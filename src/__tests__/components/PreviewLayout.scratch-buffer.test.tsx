import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import type { BufferDocument } from "../../types/buffer";

// End-to-end regression for the scratch-buffer render path the L2 gates
// did not cover. The coverage gap: every existing unit/component test
// involving the preview ran with source-backed buffers (filename ending in
// a recognized extension). When a SCRATCH buffer ("test.html" title, but
// Rust-generated <uuid>.txt filename, no source_path) became the active
// renderable buffer, contentTypeForBuffer fell to the filename and
// returned null — PreviewLayout.showsPane stayed false — the iframe was
// never inserted into the DOM. This test mounts <PreviewLayout> with such
// a buffer and asserts the iframe DOES mount and is pointed at the live
// render under writ-preview://.

// IPC layer — explicit minimal mock surface. Hoisted so vi.mock's
// top-of-file factory can reach the mock fns.
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
  // Preview IPC.
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.forceRender,
  previewRender: mocks.forceRender,
  previewClose: mocks.previewClose,
  previewGetLayout: mocks.previewGetLayout,
  previewSetLayout: mocks.previewSetLayout,
  // Surface other transitive imports touch — sidebar-store imports
  // searchBuffers; configStore imports getConfig/updateConfig (only called
  // inside its methods, not on import, but we stub them defensively).
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

// The real EditorInstance pulls CodeMirror + bufferRegistry.readContent.
// This regression is about the preview-layout decision and iframe mount,
// not the editor; stub it out. The stub still publishes the loaded buffer id
// so PreviewPane's render gate matches (the editor contract relied on by #97).
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

function scratchHtmlBuffer(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "B1",
    title: "test.html",
    // The Rust-generated scratch filename — does NOT end in .html.
    filename: "abc-uuid-1234.txt",
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    ...overrides,
  };
}

describe("PreviewLayout — scratch buffer with renderable title (regression for L2 escape, see #97 for related flash race)", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
    mocks.previewClose.mockClear();
    mocks.previewGetLayout.mockClear();
    mocks.previewSetLayout.mockClear();
    // Populate the registry as it would be at app boot.
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

  it("mounts the preview iframe for a scratch buffer titled '.html'", async () => {
    const { container } = render(() => (
      <WindowProvider windowId={9001}>
        <PreviewLayout buffer={scratchHtmlBuffer()} />
      </WindowProvider>
    ));

    // initLayout is an async effect that resolves to split for a
    // renderable buffer; PreviewPane then mounts and onMount triggers the
    // first render. The iframe element appears after the render resolves
    // (Show gates on hasRendered).
    await waitFor(
      () => {
        const iframe = container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
        expect(iframe).not.toBeNull();
        expect(iframe!.src).toMatch(
          /^writ-preview:\/\/document\/B1\?v=[1-9]\d*$/,
        );
      },
      { timeout: 2000 },
    );

    expect(mocks.forceRender).toHaveBeenCalledWith(
      9001,
      "B1",
      "html",
      expect.any(String),
    );
  });

  it("re-renders when the buffer's live text changes (debounced)", async () => {
    vi.useFakeTimers();
    try {
      render(() => (
        <WindowProvider windowId={9002}>
          <PreviewLayout buffer={scratchHtmlBuffer({ id: "B2" })} />
        </WindowProvider>
      ));

      // Let initLayout's async path + the first render resolve.
      await vi.runAllTimersAsync();
      // Drain any pending microtasks from the render promise.
      await Promise.resolve();
      await Promise.resolve();
      mocks.forceRender.mockClear();

      // Drive content through the production path: EditorInstance's
      // updateListener calls setCurrentText after every doc change.
      const win = windowRegistry.getActive();
      expect(win, "WindowProvider registered the window state").not.toBeNull();
      win!.editor.setCurrentText("<h1>hi</h1>");

      // Default debounce is 200ms.
      await vi.advanceTimersByTimeAsync(250);
      await Promise.resolve();
      await Promise.resolve();

      expect(mocks.forceRender).toHaveBeenCalledWith(9002, "B2", "html", "<h1>hi</h1>");
    } finally {
      vi.useRealTimers();
    }
  });
});
