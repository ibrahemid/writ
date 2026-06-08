import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { Show } from "solid-js";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import type { BufferDocument } from "../../types/buffer";

// Regression for #122: renaming a non-renderable scratch buffer to a
// renderable extension (.txt → .md) must activate the preview WITHOUT a
// close+reopen cycle. The escape: PreviewLayout resolved its layout exactly
// once per buffer id (the `initialized` guard) at open time. A scratch
// buffer opens non-renderable, so it was pinned to {kind:"source"} and the
// guard blocked re-resolution. EditorArea mounts PreviewLayout under a
// NON-KEYED <Show>, so the instance (and its guard) survives the rename —
// the preview never appeared until close+reopen recreated the instance.
//
// This test reproduces EditorArea's exact wiring (non-keyed Show sourced
// from bufferRegistry.activeTabs()) and drives the rename through the real
// bufferRegistry.renameBuffer store handler.

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn(),
  listHistory: vi.fn().mockResolvedValue([]),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.forceRender,
  previewRender: mocks.forceRender,
  previewClose: mocks.previewClose,
  previewGetLayout: mocks.previewGetLayout,
  previewSetLayout: mocks.previewSetLayout,
  renameBuffer: mocks.renameBuffer,
  listActiveBuffers: mocks.listActiveBuffers,
  listHistory: mocks.listHistory,
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

// The real EditorInstance pulls CodeMirror; this regression is about the
// layout decision and iframe mount, not the editor. The stub still honors the
// editor's contract that the preview pane depends on: publish the loaded
// buffer id so PreviewPane's render gate matches (see #97).
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

// Mirrors EditorArea's NON-KEYED Show: the instance persists across buffer
// object identity changes (which is what makes #122 reproducible).
function EditorAreaLike() {
  const win = useWindow();
  const activeBuffer = () => {
    const id = win.tabs.activeTabId();
    if (!id) return null;
    return bufferRegistry.activeTabs().find((b) => b.id === id) ?? null;
  };
  return (
    <Show when={activeBuffer()}>
      {(buf) => <PreviewLayout buffer={buf()} />}
    </Show>
  );
}

function scratchTxtBuffer(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "R1",
    // No recognized extension — not renderable at open.
    title: "untitled",
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

describe("PreviewLayout — rename to renderable extension activates preview (regression #122)", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
    mocks.renameBuffer.mockClear();
    mocks.listActiveBuffers.mockResolvedValue([scratchTxtBuffer()]);
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

  afterEach(async () => {
    cleanup();
    rendererRegistry.setFromIpc([]);
    mocks.listActiveBuffers.mockResolvedValue([]);
    await bufferRegistry.load();
  });

  it("mounts the preview iframe after renaming untitled → test.md (no close/reopen)", async () => {
    await bufferRegistry.load();

    const { container } = render(() => (
      <WindowProvider windowId={9101}>
        <EditorAreaLike />
      </WindowProvider>
    ));

    const win = (await import("../../stores/global/window-registry")).windowRegistry.getActive();
    expect(win).not.toBeNull();
    win!.tabs.setActiveTabId("R1");

    // Not renderable yet — no iframe.
    await Promise.resolve();
    expect(container.querySelector("iframe.preview-frame")).toBeNull();

    // Rename through the real store handler.
    await bufferRegistry.renameBuffer("R1", "test.md");
    expect(mocks.renameBuffer).toHaveBeenCalledWith("R1", "test.md");

    // Preview must now activate without any close/reopen.
    await waitFor(
      () => {
        const iframe = container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
        expect(iframe).not.toBeNull();
        expect(iframe!.src).toMatch(/^writ-preview:\/\/document\/R1\?v=[1-9]\d*$/);
      },
      { timeout: 2000 },
    );

    expect(mocks.forceRender).toHaveBeenCalledWith(
      9101,
      "R1",
      "markdown",
      expect.any(String),
    );
  });

  it("drops the preview and reclaims editor width when test.md → untitled.txt", async () => {
    mocks.listActiveBuffers.mockResolvedValue([scratchTxtBuffer({ title: "test.md" })]);
    await bufferRegistry.load();

    const { container } = render(() => (
      <WindowProvider windowId={9102}>
        <EditorAreaLike />
      </WindowProvider>
    ));

    const win = (await import("../../stores/global/window-registry")).windowRegistry.getActive();
    win!.tabs.setActiveTabId("R1");

    // Renderable .md scratch buffer: preview is active (split default).
    await waitFor(
      () => expect(container.querySelector("iframe.preview-frame")).not.toBeNull(),
      { timeout: 2000 },
    );
    const editorSlot = container.querySelector<HTMLElement>(".preview-editor-slot");
    expect(editorSlot).not.toBeNull();
    expect(editorSlot!.style.flexBasis).toMatch(/%$/);

    // Rename to a non-renderable extension: preview drops, editor reclaims
    // full width (no dangling split gap).
    await bufferRegistry.renameBuffer("R1", "untitled.txt");

    await waitFor(
      () => {
        expect(container.querySelector("iframe.preview-frame")).toBeNull();
        expect(container.querySelector(".preview-pane-slot")).toBeNull();
        expect(editorSlot!.style.flexBasis).toBe("0px");
      },
      { timeout: 2000 },
    );
  });
});
