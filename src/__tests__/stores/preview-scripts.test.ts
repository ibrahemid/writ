import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot } from "solid-js";

// The preview-scripts opt-in as the status bar reads it: unknown until read,
// set by the toggle and by each render's verdict, the answer asked for last
// winning, and read again when another program changes the file, which also
// reloads the preview showing it.

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

  it("reads a changed file's opt-in again and reloads the preview showing it", async () => {
    mocks.get.mockResolvedValueOnce(true);
    await store.load("buf-1");
    const win = windowRegistry.getActive()!;
    win.tabs.setActiveTabId("buf-1");
    const before = win.preview.forceRefreshToken();
    mocks.get.mockResolvedValue(false);

    store.refresh("buf-1");

    await vi.waitFor(() => expect(store.enabled("buf-1")).toBe(false));
    expect(mocks.set).not.toHaveBeenCalled();
    expect(win.preview.forceRefreshToken()).toBe(before + 1);
  });

  it("leaves the preview alone when it shows another file", async () => {
    const win = windowRegistry.getActive()!;
    win.tabs.setActiveTabId("buf-2");
    const before = win.preview.forceRefreshToken();
    mocks.get.mockResolvedValue(false);

    store.refresh("buf-1");

    await vi.waitFor(() => expect(store.enabled("buf-1")).toBe(false));
    expect(win.preview.forceRefreshToken()).toBe(before);
  });

  it("keeps what it knew when the read after a change fails", async () => {
    mocks.get.mockResolvedValueOnce(false);
    await store.load("buf-1");
    mocks.get.mockRejectedValue(new Error("store locked"));

    store.refresh("buf-1");

    await vi.waitFor(() => expect(mocks.get).toHaveBeenCalledTimes(2));
    expect(store.enabled("buf-1")).toBe(false);
  });

  it("shows a render's verdict over a read asked for before the render", async () => {
    let answerRead: (enabled: boolean) => void = () => {};
    mocks.get.mockReturnValue(new Promise<boolean>((resolve) => (answerRead = resolve)));
    const reading = store.load("buf-1");
    const rendered = store.issue();

    store.settle("buf-1", rendered, false);
    answerRead(true);
    await reading;

    expect(store.enabled("buf-1")).toBe(false);
  });

  it("shows a read over the verdict of a render asked for before it", async () => {
    const rendered = store.issue();
    mocks.get.mockResolvedValue(false);
    await store.load("buf-1");

    store.settle("buf-1", rendered, true);

    expect(store.enabled("buf-1")).toBe(false);
  });
});
