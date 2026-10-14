import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// The command that turns a file's own preview scripts on or off: its label
// says it toggles, it reports the state it left the file in, and a failed
// write says so instead of passing silently.

const mocks = vi.hoisted(() => ({
  getScripts: vi.fn(),
  setScripts: vi.fn(),
  toast: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  previewGetScripts: mocks.getScripts,
  previewSetScripts: mocks.setScripts,
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: mocks.toast,
}));

import { getCommand, unregisterCommand } from "../../commands/registry";
import { windowRegistry } from "../../stores/global/window-registry";
import { createWindowState } from "../../stores/window/createWindowState";
import { registerPreviewKeymap } from "../../keymap/preview";

const COMMAND_IDS = [
  "preview.cycleLayout",
  "preview.refresh",
  "preview.toggleFullscreen",
  "preview.exitFullscreen",
  "preview.swapOrientation",
  "preview.resetRatio",
  "preview.toggleRunScripts",
];

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
}

describe("preview scripts command", () => {
  let windowId = 100;
  let release: (() => void) | null = null;

  beforeEach(() => {
    windowId += 1;
    release = windowRegistry.register(createWindowState({ windowId }));
    windowRegistry.focus(windowId);
    windowRegistry.getActive()!.tabs.setActiveTabId("buf-1");
    registerPreviewKeymap();
    mocks.getScripts.mockReset();
    mocks.setScripts.mockReset();
    mocks.toast.mockReset();
  });

  afterEach(() => {
    for (const id of COMMAND_IDS) unregisterCommand(id);
    release?.();
    release = null;
  });

  it("is labelled as the toggle it is", () => {
    expect(getCommand("preview.toggleRunScripts")!.label).toBe(
      "Preview: Toggle scripts for this file",
    );
  });

  it("does not promise that an opted-in file stays off the network", () => {
    const description = getCommand("preview.toggleRunScripts")!.description ?? "";
    expect(description).not.toMatch(/network/i);
  });

  it("turns a file's scripts on, refreshes the preview and says so", async () => {
    mocks.getScripts.mockResolvedValue(false);
    mocks.setScripts.mockResolvedValue(undefined);
    const win = windowRegistry.getActive()!;
    const before = win.preview.forceRefreshToken();

    getCommand("preview.toggleRunScripts")!.execute();
    await settle();

    expect(mocks.setScripts).toHaveBeenCalledWith("buf-1", true);
    expect(win.preview.forceRefreshToken()).toBe(before + 1);
    expect(mocks.toast).toHaveBeenCalledWith("Scripts on for this file", "info");
  });

  it("says when the choice could not be recorded and leaves the preview alone", async () => {
    mocks.getScripts.mockResolvedValue(false);
    mocks.setScripts.mockRejectedValue(new Error("buffer missing"));
    const win = windowRegistry.getActive()!;
    const before = win.preview.forceRefreshToken();

    getCommand("preview.toggleRunScripts")!.execute();
    await settle();

    expect(win.preview.forceRefreshToken()).toBe(before);
    expect(mocks.toast).toHaveBeenCalledWith("Could not change scripts for this file", "error");
    expect(mocks.toast).not.toHaveBeenCalledWith("Scripts on for this file", "info");
  });
});
