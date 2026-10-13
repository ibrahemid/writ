import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot } from "solid-js";

// The stored preview-scripts opt-in as the status bar reads it: unknown until
// read, set by the toggle, and turned off when another program changes the
// file, which also reloads the preview showing it.

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  previewGetScripts: mocks.get,
  previewSetScripts: mocks.set,
}));

import { createPreviewScriptsStore } from "../../stores/global/preview-scripts";
import { windowRegistry } from "../../stores/global/window-registry";
import { createWindowState } from "../../stores/window/createWindowState";

describe("previewScriptsStore", () => {
  let windowId = 600;
  let release: (() => void) | null = null;
  let store: ReturnType<typeof createPreviewScriptsStore>;

  beforeEach(() => {
    mocks.get.mockReset();
    mocks.set.mockReset().mockResolvedValue(undefined);
    windowId += 1;
    release = windowRegistry.register(createWindowState({ windowId }));
    windowRegistry.focus(windowId);
    store = createRoot(createPreviewScriptsStore);
  });

  afterEach(() => {
    release?.();
    release = null;
  });

  it("knows nothing about a file until it reads the stored choice", async () => {
    mocks.get.mockResolvedValue(true);
    expect(store.enabled("buf-1")).toBeUndefined();
    await store.load("buf-1");
    expect(mocks.get).toHaveBeenCalledWith("buf-1");
    expect(store.enabled("buf-1")).toBe(true);
  });

  it("stays unknown when the stored choice cannot be read", async () => {
    mocks.get.mockRejectedValue(new Error("store locked"));
    await store.load("buf-1");
    expect(store.enabled("buf-1")).toBeUndefined();
  });

  it("records what the toggle stored", async () => {
    mocks.get.mockResolvedValue(false);
    await expect(store.toggle("buf-1")).resolves.toBe(true);
    expect(mocks.set).toHaveBeenCalledWith("buf-1", true);
    expect(store.enabled("buf-1")).toBe(true);
  });

  it("turns the scripts off for a field showing them on after the stored opt-in lapsed", async () => {
    mocks.get.mockResolvedValueOnce(true);
    await store.load("buf-1");
    mocks.get.mockResolvedValue(false);

    await expect(store.toggle("buf-1")).resolves.toBe(false);
    expect(mocks.set).toHaveBeenCalledWith("buf-1", false);
    expect(store.enabled("buf-1")).toBe(false);
  });

  it("records nothing when the toggle could not store the choice", async () => {
    mocks.get.mockResolvedValue(false);
    mocks.set.mockRejectedValue(new Error("buffer missing"));
    await expect(store.toggle("buf-1")).rejects.toThrow("buffer missing");
    expect(store.enabled("buf-1")).toBeUndefined();
  });

  it("turns a changed file's scripts off and reloads the preview showing it", async () => {
    const win = windowRegistry.getActive()!;
    win.tabs.setActiveTabId("buf-1");
    const before = win.preview.forceRefreshToken();

    await store.turnOff("buf-1");

    expect(mocks.set).toHaveBeenCalledWith("buf-1", false);
    expect(store.enabled("buf-1")).toBe(false);
    expect(win.preview.forceRefreshToken()).toBe(before + 1);
  });

  it("leaves the preview alone when it shows another file", async () => {
    const win = windowRegistry.getActive()!;
    win.tabs.setActiveTabId("buf-2");
    const before = win.preview.forceRefreshToken();

    await store.turnOff("buf-1");

    expect(store.enabled("buf-1")).toBe(false);
    expect(win.preview.forceRefreshToken()).toBe(before);
  });

  it("reads the stored choice back when turning it off fails", async () => {
    mocks.set.mockRejectedValue(new Error("store locked"));
    mocks.get.mockResolvedValue(false);

    await store.turnOff("buf-1");

    expect(mocks.get).toHaveBeenCalledWith("buf-1");
    expect(store.enabled("buf-1")).toBe(false);
  });
});
