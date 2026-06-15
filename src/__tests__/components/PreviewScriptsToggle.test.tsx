import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { configStore } from "../../stores/global/config";
import { defaultSplit } from "../../lib/preview-layout";
import type { BufferDocument } from "../../types/buffer";
import type { WritConfig } from "../../types/config";

const mocks = vi.hoisted(() => ({
  save: vi.fn().mockResolvedValue(undefined),
  config: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

// configStore is a global singleton; stub its read/save so the toggle's
// state and persistence are observable.
vi.spyOn(configStore, "save").mockImplementation(mocks.save);
vi.spyOn(configStore, "config").mockImplementation(mocks.config);

import PreviewScriptsToggle from "../../components/Preview/PreviewScriptsToggle";

function baseConfig(runScripts: boolean): WritConfig {
  return {
    hotkey: { toggle: "" },
    sidebar: { toggle: "", default_visible: false, position: "left", open: false },
    editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_typography: true },
    window: { width: 800, height: 600 },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "warp-dark", overrides: {} },
    commands: { usage: {} },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
    preview: {
      default_layout_html: "split",
      default_layout_markdown: "split",
      live_render_threshold_mb: 1,
      render_confirm_threshold_mb: 5,
      render_refuse_threshold_mb: 50,
      debounce_ms: 200,
      run_scripts: runScripts,
    },
  };
}

const HTML_BUFFER: BufferDocument = {
  id: "S1",
  title: "page.html",
  filename: "s-uuid.txt",
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
};

describe("PreviewScriptsToggle", () => {
  beforeEach(() => {
    mocks.save.mockReset().mockResolvedValue(undefined);
    mocks.config.mockReset().mockReturnValue(baseConfig(true));
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

  async function mount(layoutKind: "source" | "split" | "preview") {
    const { listActiveBuffers } = await import("../../services/tauri");
    (listActiveBuffers as ReturnType<typeof vi.fn>).mockResolvedValue([HTML_BUFFER]);
    await bufferRegistry.load();

    const result = render(() => (
      <WindowProvider windowId={5151}>
        <PreviewScriptsToggle />
      </WindowProvider>
    ));
    await waitFor(() => expect(windowRegistry.getActive()).not.toBeNull());
    const win = windowRegistry.getActive()!;
    win.tabs.setActiveTabId(HTML_BUFFER.id);
    win.layout.setLocal(HTML_BUFFER.id, layoutKind === "split" ? defaultSplit() : { kind: layoutKind });
    return { ...result, win };
  }

  it("is hidden in source-only layout (no preview visible)", async () => {
    const { container } = await mount("source");
    await waitFor(() => expect(windowRegistry.getActive()).not.toBeNull());
    expect(container.querySelector(".scripts-toggle")).toBeNull();
  });

  it("is shown in split layout and reflects the on state", async () => {
    const { container } = await mount("split");
    await waitFor(() => {
      const btn = container.querySelector(".scripts-toggle");
      expect(btn).not.toBeNull();
      expect(btn!.getAttribute("aria-pressed")).toBe("true");
      expect(btn!.textContent).toContain("scripts");
    });
  });

  it("reflects the off state", async () => {
    mocks.config.mockReturnValue(baseConfig(false));
    const { container } = await mount("preview");
    await waitFor(() => {
      const btn = container.querySelector<HTMLButtonElement>(".scripts-toggle")!;
      expect(btn.classList.contains("is-off")).toBe(true);
      expect(btn.getAttribute("aria-pressed")).toBe("false");
      expect(btn.textContent).toContain("scripts off");
    });
  });

  it("toggling saves config with run_scripts flipped", async () => {
    const { container } = await mount("split");
    let btn: HTMLButtonElement | null = null;
    await waitFor(() => {
      btn = container.querySelector<HTMLButtonElement>(".scripts-toggle");
      expect(btn).not.toBeNull();
    });
    fireEvent.click(btn!);
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.preview.run_scripts).toBe(false);
  });
});
