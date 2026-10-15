import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { PANES_STORAGE_KEY } from "../../stores/window/pane-store";
import type { WindowState } from "../../stores/window/createWindowState";
import type { BufferDocument } from "../../types/buffer";

// The one preview beside two panes (ADR-056): it shows the focused pane's tab,
// and its iframe is the same element through Split right, a focus change and
// Close split, since removing a writ-preview:// iframe freezes the macOS
// webview. A split kept from the last launch comes back with both panes.

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  tabs: [] as BufferDocument[],
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

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => mocks.tabs,
    buffers: () => mocks.tabs,
    historyList: () => [],
    readContent: async () => "",
  },
}));

// The editor's contract the preview reads, kept per pane: the view first,
// then the id, both cleared when the editor goes.
vi.mock("../../components/Editor/EditorInstance", async () => {
  const { createEffect, onCleanup } = await import("solid-js");
  const { EditorState } = await import("@codemirror/state");
  const { EditorView } = await import("@codemirror/view");
  const { useWindow: useWin } = await import("../../components/WindowProvider/WindowProvider");
  return {
    default: (props: { buffer: { id: string }; pane?: "left" | "right" }) => {
      const ed = useWin().editor.forPane(props.pane ?? "left");
      createEffect(() => {
        ed.registerView(new EditorView({ state: EditorState.create({ doc: "<p>x</p>" }) }));
        ed.setCurrentBufferId(props.buffer.id);
      });
      onCleanup(() => {
        ed.registerView(null);
        ed.setCurrentBufferId(null);
      });
      return <div data-testid={`editor-${props.pane ?? "left"}`} data-buffer={props.buffer.id} />;
    },
  };
});

import PreviewLayout from "../../components/Preview/PreviewLayout";
import { useActiveBuffer } from "../../lib/use-active-buffer";

function htmlBuffer(id: string): BufferDocument {
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

function frame(container: HTMLElement): HTMLIFrameElement | null {
  return container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
}

function mount() {
  let win!: WindowState;
  function Window() {
    win = useWindow();
    const active = useActiveBuffer();
    return <PreviewLayout buffer={active()} />;
  }
  const result = render(() => (
    <WindowProvider windowId={7356}>
      <Window />
    </WindowProvider>
  ));
  return { ...result, win: () => win };
}

describe("the preview beside two panes", () => {
  beforeAll(() => {
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    Element.prototype.scrollIntoView = vi.fn();
  });

  beforeEach(() => {
    localStorage.clear();
    mocks.tabs.length = 0;
    mocks.tabs.push(htmlBuffer("H1"), htmlBuffer("H2"));
    rendererRegistry.setFromIpc([
      {
        content_type: "html",
        capabilities: { supports_live_render: true, supports_print: true, max_safe_document_bytes: 50 * 1024 * 1024 },
      },
    ]);
  });

  afterEach(() => {
    cleanup();
    rendererRegistry.setFromIpc([]);
  });

  it("follows the focused pane with the same iframe throughout", async () => {
    const { container, win } = mount();
    win().tabs.setActiveTabId("H1");
    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/));
    const original = frame(container);

    win().tabs.setActiveTabId("H2");
    win().tabs.moveToOtherPane();
    await waitFor(() => expect(container.querySelector('[data-testid="editor-right"]')).not.toBeNull());
    expect(win().panes.focused()).toBe("right");
    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H2\?v=[1-9]\d*$/));
    expect(frame(container)).toBe(original);

    win().tabs.focusOtherPane();
    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/));
    expect(frame(container)).toBe(original);

    win().tabs.focusOtherPane();
    win().tabs.closeSplit();
    await waitFor(() => expect(container.querySelector('[data-testid="editor-right"]')).toBeNull());
    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H2\?v=[1-9]\d*$/));
    expect(frame(container)).toBe(original);
    expect(original!.isConnected).toBe(true);
  });

  it("keeps the iframe through Split right on one file", async () => {
    const { container, win } = mount();
    win().tabs.setActiveTabId("H1");
    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/));
    const original = frame(container);

    win().tabs.splitRight();
    await waitFor(() => expect(container.querySelector('[data-testid="editor-right"]')).not.toBeNull());
    expect(container.querySelector('[data-testid="editor-right"]')!.getAttribute("data-buffer")).toBe("H1");
    expect(frame(container)).toBe(original);
    expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/);
  });

  it("shows both panes again when the last launch left a split", async () => {
    localStorage.setItem(
      PANES_STORAGE_KEY,
      JSON.stringify({ v: 1, ratio: 0.4, focused: "right", right: ["H2"], both: [], active: { left: "H1", right: "H2" } }),
    );
    const { container, win } = mount();
    win().tabs.restorePanes();

    await waitFor(() => expect(container.querySelector('[data-testid="editor-right"]')).not.toBeNull());
    expect(container.querySelector('[data-testid="editor-left"]')!.getAttribute("data-buffer")).toBe("H1");
    expect(container.querySelector('[data-testid="editor-right"]')!.getAttribute("data-buffer")).toBe("H2");
    expect(container.querySelectorAll(".pane-group .tabbar")).toHaveLength(2);
    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H2\?v=[1-9]\d*$/));
  });
});
