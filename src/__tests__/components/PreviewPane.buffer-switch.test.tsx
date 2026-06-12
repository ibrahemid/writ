import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import type { BufferDocument } from "../../types/buffer";

// Regression for #97: the stale-cache flash on tab switch between two
// renderable buffers. PreviewPane is a single persistent instance (recreating
// a loaded writ-preview:// iframe freezes the macOS webview, #124), so on a
// switch props.buffer.id flips reactively while the editor is still mid-load
// on the OUTGOING buffer. Two defects produced the flash:
//   1. doRender read the shared currentText signal and could render the
//      incoming id with the outgoing buffer's text, caching the wrong HTML
//      under the incoming id (cross-buffer cache pollution).
//   2. the iframe src retargeted to the incoming id with a stale version
//      before any fresh render landed, painting that id's (polluted/empty)
//      cache slot for a frame.
// The fix: editor-store publishes currentBufferId; doRender refuses to render
// when the loaded id does not match this pane's id; and the iframe src only
// advances on a successful, correctly-attributed render.

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

import PreviewPane from "../../components/Preview/PreviewPane";

function buffer(id: string, overrides: Partial<BufferDocument> = {}): BufferDocument {
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
    ...overrides,
  };
}

function iframeSrc(container: HTMLElement): string | null {
  return container.querySelector<HTMLIFrameElement>("iframe.preview-frame")?.src ?? null;
}

describe("PreviewPane — buffer switch (regression #97)", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it("does not flash the outgoing buffer and never caches it under the incoming id", async () => {
    const [buf, setBuf] = createSignal(buffer("A"));

    const { container } = render(() => (
      <WindowProvider windowId={9301}>
        <PreviewPane buffer={buf()} contentType="html" isActive={true} />
      </WindowProvider>
    ));

    const win = windowRegistry.getActive();
    expect(win).not.toBeNull();

    // Editor loads buffer A.
    win!.editor.setCurrentText("<body>A</body>");
    win!.editor.setCurrentBufferId("A");

    await waitFor(() => {
      expect(iframeSrc(container)).toMatch(/writ-preview:\/\/document\/A\?v=[1-9]\d*$/);
    });
    expect(mocks.forceRender).toHaveBeenCalledWith(9301, "A", "html", "<body>A</body>", "dark");
    const srcShowingA = iframeSrc(container);
    mocks.forceRender.mockClear();

    // Tab switch: the buffer prop flips to B, but the editor is still mid-load
    // and reports A. The pane must NOT retarget the iframe to B yet, and must
    // NOT render B's id with A's text.
    setBuf(buffer("B"));
    // Even a force-refresh fired in this window must be refused (the guard).
    win!.preview.requestForceRefresh();
    await Promise.resolve();
    await Promise.resolve();

    expect(mocks.forceRender).not.toHaveBeenCalled();
    // No flash: the iframe still shows A's last good render.
    expect(iframeSrc(container)).toBe(srcShowingA);

    // Editor finishes loading B.
    win!.editor.setCurrentText("<body>B</body>");
    win!.editor.setCurrentBufferId("B");

    await waitFor(() => {
      expect(iframeSrc(container)).toMatch(/writ-preview:\/\/document\/B\?v=[1-9]\d*$/);
    });
    expect(mocks.forceRender).toHaveBeenCalledWith(9301, "B", "html", "<body>B</body>", "dark");
    // B was never rendered with A's text under either id.
    expect(mocks.forceRender).not.toHaveBeenCalledWith(9301, "B", "html", "<body>A</body>", "dark");
  });

  it("renders the incoming buffer on switch even when both buffers hold identical text", async () => {
    const [buf, setBuf] = createSignal(buffer("A"));

    const { container } = render(() => (
      <WindowProvider windowId={9302}>
        <PreviewPane buffer={buf()} contentType="html" isActive={true} />
      </WindowProvider>
    ));

    const win = windowRegistry.getActive();
    win!.editor.setCurrentText("<body>same</body>");
    win!.editor.setCurrentBufferId("A");

    await waitFor(() =>
      expect(iframeSrc(container)).toMatch(/document\/A\?v=[1-9]\d*$/),
    );
    mocks.forceRender.mockClear();

    // Switch to B with byte-identical content: the shared currentText signal
    // does not change, so a text-driven trigger would never fire. The buffer-id
    // trigger must still force a render for B.
    setBuf(buffer("B"));
    win!.editor.setCurrentBufferId("B");

    await waitFor(() =>
      expect(iframeSrc(container)).toMatch(/document\/B\?v=[1-9]\d*$/),
    );
    expect(mocks.forceRender).toHaveBeenCalledWith(9302, "B", "html", "<body>same</body>", "dark");
  });

  it("discards an in-flight render that completes after the buffer switched away", async () => {
    // A's render is held open; every later render resolves immediately. This
    // reproduces the completion race: the render started for A finishes only
    // after the user has switched to B. Committing it would retarget the iframe
    // to B's slot under A's stale completion (flash) and poison the dedup.
    let resolveA: (value: { kind: "rendered"; used_fallback_stylesheet: boolean; parser_warnings: string[] }) => void;
    const aPending = new Promise<{ kind: "rendered"; used_fallback_stylesheet: boolean; parser_warnings: string[] }>(
      (resolve) => { resolveA = resolve; },
    );
    mocks.forceRender
      .mockImplementationOnce(() => aPending)
      .mockResolvedValue({ kind: "rendered", used_fallback_stylesheet: true, parser_warnings: [] });

    const [buf, setBuf] = createSignal(buffer("A"));
    const { container } = render(() => (
      <WindowProvider windowId={9303}>
        <PreviewPane buffer={buf()} contentType="html" isActive={true} />
      </WindowProvider>
    ));

    const win = windowRegistry.getActive();
    win!.editor.setCurrentText("<body>A</body>");
    win!.editor.setCurrentBufferId("A");
    await Promise.resolve();
    await Promise.resolve();
    expect(mocks.forceRender).toHaveBeenCalledWith(9303, "A", "html", "<body>A</body>", "dark");

    // Switch to B while A's render is still pending; B renders to completion.
    setBuf(buffer("B"));
    win!.editor.setCurrentText("<body>B</body>");
    win!.editor.setCurrentBufferId("B");
    await waitFor(() =>
      expect(iframeSrc(container)).toMatch(/document\/B\?v=[1-9]\d*$/),
    );
    const srcAfterB = iframeSrc(container);

    // A's stale render finally resolves — it must be discarded.
    resolveA!({ kind: "rendered", used_fallback_stylesheet: true, parser_warnings: [] });
    await Promise.resolve();
    await Promise.resolve();
    expect(iframeSrc(container)).toBe(srcAfterB);

    // The dedup baseline must hold B's text, not A's: editing B's live text to
    // A's old content still triggers a render (wrongly skipped if poisoned).
    mocks.forceRender.mockClear();
    win!.editor.setCurrentText("<body>A</body>");
    await waitFor(() =>
      expect(mocks.forceRender).toHaveBeenCalledWith(9303, "B", "html", "<body>A</body>", "dark"),
    );
  });
});
