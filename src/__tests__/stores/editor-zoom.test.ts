import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import { configStore } from "../../stores/global/config";
import { editorZoom } from "../../stores/global/editor-zoom";
import type { WritConfig } from "../../types/config";

const MOCK_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: { toggle: "CmdOrCtrl+S", default_visible: false, position: "left", open: false },
  editor: { font_family: "JetBrains Mono", font_size: 16, word_wrap: true, tab_size: 4, autosave_debounce_ms: 500, markdown_typography: true, markdown_editing: true },
  window: { width: 1200, height: 800 },
  keybindings: {},
  history: { max_entries: 1000 },
  storage: { path: "~/.writ" },
  theme: { preset: "warp-dark", overrides: {} },
  commands: { usage: {} },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: { enabled: false, preset: "ollama", base_url: "http://localhost:11434/v1", model: "", consented_hosts: [] },
  spelling: { enabled: false, dialect: "american", ignored_words: [] },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "split",
    live_render_threshold_mb: 1,
    render_confirm_threshold_mb: 5,
    render_refuse_threshold_mb: 50,
    debounce_ms: 200,
    run_scripts: true,
  },
};

function withFontSize(size: number): WritConfig {
  return { ...MOCK_CONFIG, editor: { ...MOCK_CONFIG.editor, font_size: size } };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("editorZoom", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    editorZoom.resetWheelThrottle();
    await configStore.save(withFontSize(16));
  });

  it("steps the editor font size up and down by one", () => {
    editorZoom.zoomIn();
    expect(editorZoom.fontSize()).toBe(17);

    editorZoom.zoomOut();
    editorZoom.zoomOut();
    expect(editorZoom.fontSize()).toBe(15);
  });

  it("clamps at the maximum when zooming in", async () => {
    await configStore.save(withFontSize(72));
    editorZoom.zoomIn();
    expect(editorZoom.fontSize()).toBe(72);
  });

  it("clamps at the minimum when zooming out", async () => {
    await configStore.save(withFontSize(8));
    editorZoom.zoomOut();
    expect(editorZoom.fontSize()).toBe(8);
  });

  it("resets to the default size", async () => {
    await configStore.save(withFontSize(28));
    editorZoom.reset();
    expect(editorZoom.fontSize()).toBe(14);
  });

  it("zooms in on scroll up and out on scroll down, device-independently", () => {
    // Mouse wheel (line mode, deltaY ~ -3) and trackpad (pixel mode) both step
    // by one because the step follows the sign, not the magnitude.
    editorZoom.handleWheel(-3, 1000);
    expect(editorZoom.fontSize()).toBe(17);

    editorZoom.handleWheel(120, 2000);
    expect(editorZoom.fontSize()).toBe(16);
  });

  it("throttles rapid wheel events so inertia does not blast the range", () => {
    editorZoom.handleWheel(-50, 5000);
    expect(editorZoom.fontSize()).toBe(17);

    // Within the throttle window — ignored.
    editorZoom.handleWheel(-50, 5010);
    expect(editorZoom.fontSize()).toBe(17);

    // Past the window — steps again.
    editorZoom.handleWheel(-50, 5100);
    expect(editorZoom.fontSize()).toBe(18);
  });

  it("ignores a zero-delta wheel event", () => {
    editorZoom.handleWheel(0, 9000);
    expect(editorZoom.fontSize()).toBe(16);
  });

  it("mirrors the live font size onto the editor CSS variable", async () => {
    await configStore.save(withFontSize(24));
    await flush();
    expect(
      document.documentElement.style.getPropertyValue("--writ-editor-font-size"),
    ).toBe("24px");
  });
});
