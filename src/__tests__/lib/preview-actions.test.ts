import { describe, it, expect, vi, beforeEach } from "vitest";
import type { WritConfig } from "../../types/config";

const mocks = vi.hoisted(() => ({
  save: vi.fn().mockResolvedValue(undefined),
  config: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: mocks.config,
    save: mocks.save,
  },
}));

import { toggleRunScripts } from "../../lib/preview-actions";

function configWith(runScripts: boolean): WritConfig {
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

describe("toggleRunScripts", () => {
  beforeEach(() => {
    mocks.save.mockReset().mockResolvedValue(undefined);
    mocks.config.mockReset();
  });

  it("saves the config with run_scripts flipped on→off", async () => {
    mocks.config.mockReturnValue(configWith(true));
    await toggleRunScripts(() => {});
    expect(mocks.save).toHaveBeenCalledTimes(1);
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.preview.run_scripts).toBe(false);
  });

  it("flips off→on", async () => {
    mocks.config.mockReturnValue(configWith(false));
    await toggleRunScripts(() => {});
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.preview.run_scripts).toBe(true);
  });

  it("preserves the rest of the config (only run_scripts changes)", async () => {
    mocks.config.mockReturnValue(configWith(true));
    await toggleRunScripts(() => {});
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.preview.debounce_ms).toBe(200);
    expect(saved.editor.font_size).toBe(14);
    expect(saved.theme.preset).toBe("warp-dark");
  });

  it("calls onApplied after the save resolves", async () => {
    mocks.config.mockReturnValue(configWith(true));
    const order: string[] = [];
    mocks.save.mockImplementation(async () => {
      order.push("save");
    });
    await toggleRunScripts(() => order.push("applied"));
    expect(order).toEqual(["save", "applied"]);
  });

  it("does NOT call onApplied when the save rejects (and propagates)", async () => {
    mocks.config.mockReturnValue(configWith(true));
    mocks.save.mockRejectedValue(new Error("disk full"));
    const onApplied = vi.fn();
    await expect(toggleRunScripts(onApplied)).rejects.toThrow("disk full");
    expect(onApplied).not.toHaveBeenCalled();
  });
});
