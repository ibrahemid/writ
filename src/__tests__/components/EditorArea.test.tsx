import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createSignal, type Accessor } from "solid-js";
import { render, cleanup } from "@solidjs/testing-library";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";
import type { BufferDocument } from "../../types/buffer";

const mocks = vi.hoisted(() => ({
  config: vi.fn(),
  currentText: vi.fn(() => "one two three"),
  activeBuffer: (() => ({ id: "b1" }) as unknown as BufferDocument | null) as Accessor<
    BufferDocument | null
  >,
}));

// The pane, the overlays and the bar each own their own IPC surface; this test
// is about which of them EditorArea mounts, so they are stubbed to markers.
vi.mock("../../components/Preview/PreviewLayout", () => ({
  default: () => <div data-testid="preview-layout" />,
}));
vi.mock("../../components/Toolbar/Toolbar", () => ({
  default: () => <div data-testid="toolbar" />,
}));
vi.mock("../../components/Editor/TabBar", () => ({
  default: () => <div data-testid="tabbar" />,
}));
vi.mock("../../components/Find/FindOverlay", () => ({ default: () => null }));
vi.mock("../../components/Editor/SpellingPreview", () => ({ default: () => null }));
vi.mock("../../components/Editor/StatusBar", () => ({
  default: () => <div data-testid="statusbar" />,
}));
vi.mock("../../lib/use-active-buffer", () => ({ useActiveBuffer: () => mocks.activeBuffer }));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      currentText: mocks.currentText,
      isRemovedOnDisk: () => false,
      noteFileState: () => "present" as const,
    },
  }),
}));

vi.spyOn(configStore, "config").mockImplementation(mocks.config);

import EditorArea from "../../components/Editor/EditorArea";
import { findStore } from "../../stores/global/find-store";

function configWith(statusBar: boolean): WritConfig {
  return {
    hotkey: { toggle: "" },
    sidebar: { toggle: "", default_visible: false, position: "left", open: true, width: 240 },
    panel: { open: false, width: 240 },
    first_run: { hint_dismissed: false },
    editor: {
      font_family: "monospace",
      font_size: 16,
      word_wrap: true,
      tab_size: 2,
      autosave_debounce_ms: 300,
      markdown_typography: true,
      markdown_editing: true,
      status_bar: statusBar,
    },
    window: { width: 800, height: 600, maximized: false },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "writ-light", overrides: {} },
    appearance: { polarity: "system", accent: "pine", prose_face: "system" },
    commands: { usage: {} },
    preview: {
      default_layout_html: "split",
      default_layout_markdown: "split",
      live_render_threshold_mb: 1,
      render_confirm_threshold_mb: 5,
      render_refuse_threshold_mb: 50,
      debounce_ms: 200,
      run_scripts: true,
    },
    workspace: { root: null },
    inbox: { path: null, focus: true },
    updater: { auto_check: true },
    ai: {
      enabled: false,
      preset: "ollama",
      base_url: "http://localhost:11434/v1",
      model: "",
      consented_hosts: [],
    },
    spelling: { enabled: false, dialect: "american", ignored_words: [] },
  };
}

describe("EditorArea", () => {
  beforeEach(() => {
    mocks.config.mockReturnValue(configWith(false));
    mocks.activeBuffer = () => ({ id: "b1" }) as unknown as BufferDocument;
  });

  afterEach(() => {
    cleanup();
    findStore.close();
  });

  it("hides the status bar by default", () => {
    const { container } = render(() => <EditorArea />);
    expect(container.querySelector("[data-testid='statusbar']")).toBeNull();
  });

  it("shows the word count at the top right of the canvas instead", () => {
    const { container } = render(() => <EditorArea />);
    const count = container.querySelector(".editor-wordcount");
    expect(count).not.toBeNull();
    expect(count!.textContent).toBe("3 words");
    // Inside the canvas, beside the preview pane, not in a bar of its own.
    expect(count!.parentElement?.classList.contains("editor-content")).toBe(true);
  });

  it("shows the bar and drops the floating count when the setting is on", () => {
    mocks.config.mockReturnValue(configWith(true));
    const { container } = render(() => <EditorArea />);
    expect(container.querySelector("[data-testid='statusbar']")).not.toBeNull();
    expect(container.querySelector(".editor-wordcount")).toBeNull();
  });

  it("seats the tab strip between the toolbar and the canvas", () => {
    const { container } = render(() => <EditorArea />);
    const area = container.querySelector(".editor-area")!;
    expect(Array.from(area.children).map((el) => el.getAttribute("data-testid") ?? el.className))
      .toEqual(["toolbar", "tabbar", "editor-content"]);
  });

  it("gives the toast stack its clearance from the root, since it is not a child", () => {
    const root = document.documentElement;
    const { unmount } = render(() => <EditorArea />);
    expect(root.style.getPropertyValue("--writ-toast-bottom")).toBe("16px");
    unmount();
    expect(root.style.getPropertyValue("--writ-toast-bottom")).toBe("");
  });

  it("raises the toast clearance over the status bar when the bar is on", () => {
    mocks.config.mockReturnValue(configWith(true));
    render(() => <EditorArea />);
    expect(document.documentElement.style.getPropertyValue("--writ-toast-bottom")).toBe("40px");
  });

  it("keeps the preview pane mounted in both states", () => {
    // #124: removing a loaded writ-preview:// iframe freezes the webview, so
    // the pane is never behind a Show.
    for (const statusBar of [false, true]) {
      mocks.config.mockReturnValue(configWith(statusBar));
      const { container, unmount } = render(() => <EditorArea />);
      expect(container.querySelector("[data-testid='preview-layout']")).not.toBeNull();
      unmount();
    }
  });

  it("hides the word count when no note is open", () => {
    mocks.activeBuffer = () => null;
    const { container } = render(() => <EditorArea />);
    expect(container.querySelector(".editor-wordcount")).toBeNull();
  });

  it("closes the find overlay once the active buffer closes", () => {
    const [buffer, setBuffer] = createSignal<BufferDocument | null>(
      { id: "b1" } as unknown as BufferDocument,
    );
    mocks.activeBuffer = buffer;
    render(() => <EditorArea />);
    findStore.open();
    expect(findStore.isOpen()).toBe(true);

    setBuffer(null);
    expect(findStore.isOpen()).toBe(false);
  });
});
