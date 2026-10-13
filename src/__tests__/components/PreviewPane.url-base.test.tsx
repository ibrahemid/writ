import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { setPreviewUrlPrefix } from "../../stores/global/preview-url";
import type { BufferDocument } from "../../types/buffer";

// The preview URL base comes from the host at startup. A pane that mounts
// before it arrives parks on about:blank; when it arrives, the pane renders
// the buffer it is showing instead of waiting for the text to change.

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.forceRender,
  previewRender: mocks.forceRender,
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import PreviewPane from "../../components/Preview/PreviewPane";
import { configStore } from "../../stores/global/config";

const DEBOUNCE_MS = configStore.config().preview.debounce_ms;

function buffer(id: string): BufferDocument {
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
  };
}

function iframeSrc(container: HTMLElement): string | null {
  return container.querySelector<HTMLIFrameElement>("iframe.preview-frame")?.src ?? null;
}

describe("PreviewPane and the host's preview base", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
  });

  afterEach(() => {
    cleanup();
    setPreviewUrlPrefix("writ-preview://");
  });

  it("renders the shown buffer once the base arrives after mount", async () => {
    setPreviewUrlPrefix(null);
    const { container } = render(() => (
      <WindowProvider windowId={9400}>
        <PreviewPane buffer={buffer("A")} contentType="html" isActive={true} />
      </WindowProvider>
    ));
    const win = windowRegistry.getActive()!;
    win.editor.setCurrentText("<body>A</body>");
    win.editor.setCurrentBufferId("A");
    // Past the live-render debounce, so no pending edit render is left to
    // pick the base up by chance.
    await new Promise((resolve) => setTimeout(resolve, 3 * DEBOUNCE_MS));

    expect(mocks.forceRender).not.toHaveBeenCalled();
    expect(iframeSrc(container)).toBe("about:blank");

    setPreviewUrlPrefix("writ-preview://");

    await waitFor(() => {
      expect(iframeSrc(container)).toMatch(/^writ-preview:\/\/document\/A\?v=1$/);
    });
    expect(mocks.forceRender).toHaveBeenCalledTimes(1);
  });

  it("builds the document URL from the Windows base when the host gives it", async () => {
    setPreviewUrlPrefix("http://writ-preview.");
    const { container } = render(() => (
      <WindowProvider windowId={9401}>
        <PreviewPane buffer={buffer("B")} contentType="html" isActive={true} />
      </WindowProvider>
    ));
    const win = windowRegistry.getActive()!;
    win.editor.setCurrentText("<body>B</body>");
    win.editor.setCurrentBufferId("B");

    await waitFor(() => {
      expect(iframeSrc(container)).toMatch(/^http:\/\/writ-preview\.document\/B\?v=1$/);
    });
  });
});
