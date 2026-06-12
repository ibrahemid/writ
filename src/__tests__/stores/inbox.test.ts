import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  pickInboxFolder: vi.fn(),
  clearInbox: vi.fn().mockResolvedValue(undefined),
  getInboxPath: vi.fn(),
  showAndFocusWindow: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: vi.fn() },
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: vi.fn() },
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import { inboxStore } from "../../stores/global/inbox";
import {
  pickInboxFolder,
  clearInbox,
  getInboxPath,
  showAndFocusWindow,
} from "../../services/tauri";
import { windowRegistry } from "../../stores/global/window-registry";
import { configStore } from "../../stores/global/config";
import { showToast } from "../../components/Notifications/Toast";
import type { WritConfig } from "../../types/config";

const mockedPick = vi.mocked(pickInboxFolder);
const mockedClear = vi.mocked(clearInbox);
const mockedGetPath = vi.mocked(getInboxPath);
const mockedFocus = vi.mocked(showAndFocusWindow);
const mockedGetActive = vi.mocked(windowRegistry.getActive);
const mockedConfig = vi.mocked(configStore.config);
const mockedToast = vi.mocked(showToast);

function configWithFocus(focus: boolean): WritConfig {
  return { inbox: { path: "/inbox", focus } } as WritConfig;
}

function fakeWindow() {
  const openFile = vi.fn().mockResolvedValue({ id: "buf-1" });
  return { win: { tabs: { openFile } } as never, openFile };
}

describe("inboxStore", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    mockedClear.mockResolvedValue(undefined);
    mockedFocus.mockResolvedValue(undefined);
    mockedConfig.mockReturnValue(configWithFocus(true));
    await inboxStore.stopWatching();
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("hydrate restores the persisted inbox path", async () => {
    mockedGetPath.mockResolvedValue("/inbox");
    await inboxStore.hydrate();
    expect(inboxStore.path()).toBe("/inbox");
  });

  it("hydrate leaves path null when nothing persisted", async () => {
    mockedGetPath.mockResolvedValue(null);
    await inboxStore.hydrate();
    expect(inboxStore.path()).toBeNull();
  });

  it("watchFolder sets the path from the picker", async () => {
    mockedPick.mockResolvedValue("/picked");
    const path = await inboxStore.watchFolder();
    expect(path).toBe("/picked");
    expect(inboxStore.path()).toBe("/picked");
  });

  it("watchFolder keeps existing state when the picker is cancelled", async () => {
    mockedPick.mockResolvedValue(null);
    const path = await inboxStore.watchFolder();
    expect(path).toBeNull();
    expect(inboxStore.path()).toBeNull();
  });

  it("stopWatching clears the path", async () => {
    mockedPick.mockResolvedValue("/inbox");
    await inboxStore.watchFolder();

    await inboxStore.stopWatching();

    expect(inboxStore.path()).toBeNull();
    expect(mockedClear).toHaveBeenCalled();
  });

  it("an arrival opens the file through the active window's tab open path", async () => {
    const { win, openFile } = fakeWindow();
    mockedGetActive.mockReturnValue(win);

    await inboxStore.handleFileArrived("/inbox/report.md", 1_000);

    expect(openFile).toHaveBeenCalledWith("/inbox/report.md");
  });

  it("focuses the window after opening when inbox.focus is true", async () => {
    const { win } = fakeWindow();
    mockedGetActive.mockReturnValue(win);
    mockedConfig.mockReturnValue(configWithFocus(true));

    await inboxStore.handleFileArrived("/inbox/report.md", 2_000_000);

    expect(mockedFocus).toHaveBeenCalledTimes(1);
  });

  it("does not focus the window when inbox.focus is false", async () => {
    const { win } = fakeWindow();
    mockedGetActive.mockReturnValue(win);
    mockedConfig.mockReturnValue(configWithFocus(false));

    await inboxStore.handleFileArrived("/inbox/report.md", 3_000_000);

    expect(mockedFocus).not.toHaveBeenCalled();
  });

  it("does not focus the window when the open fails", async () => {
    const openFile = vi.fn().mockRejectedValue(new Error("gate rejected"));
    mockedGetActive.mockReturnValue({ tabs: { openFile } } as never);

    await inboxStore.handleFileArrived("/inbox/report.md", 4_000_000);

    expect(mockedFocus).not.toHaveBeenCalled();
  });

  it("survives an arrival with no active window", async () => {
    mockedGetActive.mockReturnValue(null);

    await expect(
      inboxStore.handleFileArrived("/inbox/report.md", 5_000_000),
    ).resolves.toBeUndefined();
  });

  it("caps a burst at 3 opens and collapses the rest into one toast", async () => {
    const { win, openFile } = fakeWindow();
    mockedGetActive.mockReturnValue(win);

    const base = 10_000_000;
    for (let i = 0; i < 5; i++) {
      await inboxStore.handleFileArrived(`/inbox/file-${i}.md`, base + i);
    }

    expect(openFile).toHaveBeenCalledTimes(3);
    expect(mockedToast).not.toHaveBeenCalled();

    vi.runOnlyPendingTimers();

    expect(mockedToast).toHaveBeenCalledTimes(1);
    expect(mockedToast).toHaveBeenCalledWith("2 new files in inbox", "info");
  });

  it("uses singular toast copy for a single overflow file", async () => {
    const { win } = fakeWindow();
    mockedGetActive.mockReturnValue(win);

    const base = 20_000_000;
    for (let i = 0; i < 4; i++) {
      await inboxStore.handleFileArrived(`/inbox/file-${i}.md`, base + i);
    }

    vi.runOnlyPendingTimers();

    expect(mockedToast).toHaveBeenCalledWith("1 new file in inbox", "info");
  });

  it("opens again once the burst window has passed", async () => {
    const { win, openFile } = fakeWindow();
    mockedGetActive.mockReturnValue(win);

    const base = 30_000_000;
    for (let i = 0; i < 3; i++) {
      await inboxStore.handleFileArrived(`/inbox/file-${i}.md`, base);
    }
    await inboxStore.handleFileArrived("/inbox/late.md", base + 2_001);

    expect(openFile).toHaveBeenCalledTimes(4);
    expect(openFile).toHaveBeenLastCalledWith("/inbox/late.md");
  });
});
