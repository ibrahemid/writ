import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import type { BufferDocument } from "../../types/buffer";

// A tab whose open has not answered yet (a restored tab waiting on the size
// question, or one whose open was refused) has no document: the editor drops
// its view and shows the tab's pane in its place. The preview beside it shows
// that tab as empty, never the note it was showing before.

const mocks = vi.hoisted(() => ({
  render: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
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
import { showInEditor } from "../fixtures/editor-view";

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
    encoding: "UTF-8",
  };
}

function iframeSrc(container: HTMLElement): string | null {
  return container.querySelector<HTMLIFrameElement>("iframe.preview-frame")?.src ?? null;
}

/** What each render was asked for: the buffer, the content type and the text. */
function rendered(): unknown[][] {
  return mocks.render.mock.calls.map((call) => call.slice(0, 3));
}

describe("PreviewPane when the editor holds no document", () => {
  beforeEach(() => {
    mocks.render.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it("shows the tab it switched to as empty, not the note before it", async () => {
    const [shown, setShown] = createSignal(buffer("A"));
    const { container } = render(() => (
      <WindowProvider windowId={9720}>
        <PreviewPane buffer={shown()} contentType="html" isActive={true} />
      </WindowProvider>
    ));
    const win = windowRegistry.getActive()!;
    showInEditor(win, "A", "<body>A</body>");
    await waitFor(() => expect(iframeSrc(container)).toMatch(/document\/A\?v=1$/));
    mocks.render.mockClear();

    // EditorInstance.showUnreadPane: the view is dropped and nothing is loaded.
    setShown(buffer("B"));
    win.editor.registerView(null);
    win.editor.setCurrentBufferId(null);

    await waitFor(() => expect(iframeSrc(container)).toMatch(/document\/B\?v=2$/));
    expect(rendered()).toEqual([["B", "html", ""]]);
  });

  it("renders the tab's own text once its open answers", async () => {
    const [shown, setShown] = createSignal(buffer("A"));
    const { container } = render(() => (
      <WindowProvider windowId={9721}>
        <PreviewPane buffer={shown()} contentType="html" isActive={true} />
      </WindowProvider>
    ));
    const win = windowRegistry.getActive()!;
    showInEditor(win, "A", "<body>A</body>");
    await waitFor(() => expect(iframeSrc(container)).toMatch(/document\/A\?v=1$/));

    setShown(buffer("B"));
    win.editor.registerView(null);
    win.editor.setCurrentBufferId(null);
    await waitFor(() => expect(iframeSrc(container)).toMatch(/document\/B\?v=2$/));
    mocks.render.mockClear();

    showInEditor(win, "B", "<body>B</body>");
    await waitFor(() => expect(iframeSrc(container)).toMatch(/document\/B\?v=3$/));
    expect(rendered()).toEqual([["B", "html", "<body>B</body>"]]);
  });
});
