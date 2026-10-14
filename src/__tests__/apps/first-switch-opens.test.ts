import { describe, it, expect, vi, beforeEach } from "vitest";

// NV-34: switching an app on changed nothing a person could see. The first
// switch-on of an app with a surface in the window opens it, once, when
// Settings closes; Open does it any time.

const h = vi.hoisted(() => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  chatShow: vi.fn(),
  panelShow: vi.fn(),
  graphOpen: vi.fn(),
  sidebarShow: vi.fn(),
  openActivity: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({ updateConfig: h.updateConfig }));
vi.mock("../../components/Activity/ActivityPanel", () => ({ openActivity: h.openActivity }));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      chatPanel: { show: h.chatShow },
      rightPanel: { show: h.panelShow },
      folderGraph: { open: h.graphOpen },
      sidebar: { show: h.sidebarShow },
    }),
  },
}));

import { configStore } from "../../stores/global/config";
import {
  canOpenApp,
  dueApps,
  openAppSurface,
  openDueApps,
  resetDueApps,
  switchAppFromSettings,
} from "../../commands/app-surfaces";
import type { AppId } from "../../types/config";

beforeEach(async () => {
  const fresh = configStore.config();
  await configStore.save({
    ...fresh,
    ai: { ...fresh.ai, chat: { ...fresh.ai.chat, enabled: false }, rewrite: { enabled: false } },
    mcp: { ...fresh.mcp, enabled: false },
    apps: { connections: false, graph: false, tags: false, offers: true, offered: [], opened: [] },
  });
  resetDueApps();
  vi.clearAllMocks();
});

describe("Open", () => {
  it.each([
    ["chat", "chatShow"],
    ["connections", "panelShow"],
    ["graph", "graphOpen"],
    ["tags", "sidebarShow"],
    ["programs", "openActivity"],
  ] as const)("shows %s", (app, shown) => {
    expect(openAppSurface(app)).toBe(true);
    expect(h[shown]).toHaveBeenCalledTimes(1);
  });

  it("has nothing to show for Rewrite, which acts on a selection", () => {
    expect(canOpenApp("rewrite")).toBe(false);
    expect(openAppSurface("rewrite")).toBe(false);
  });
});

describe("the first switch-on from Settings", () => {
  it("opens the app once Settings closes, and records it", async () => {
    await switchAppFromSettings("graph", true);
    expect(h.graphOpen).not.toHaveBeenCalled();
    expect(dueApps()).toEqual(["graph"]);

    openDueApps();
    expect(h.graphOpen).toHaveBeenCalledTimes(1);
    expect(configStore.wasAppOpened("graph")).toBe(true);
    const written = h.updateConfig.mock.lastCall![0];
    expect(written.apps.opened).toEqual(["graph"]);
  });

  it("opens nothing on a later switch-on", async () => {
    await switchAppFromSettings("connections", true);
    openDueApps();
    await switchAppFromSettings("connections", false);
    await switchAppFromSettings("connections", true);
    h.panelShow.mockClear();
    openDueApps();
    expect(h.panelShow).not.toHaveBeenCalled();
  });

  it("opens nothing for an app switched off again before Settings closed", async () => {
    await switchAppFromSettings("chat", true);
    await switchAppFromSettings("chat", false);
    openDueApps();
    expect(h.chatShow).not.toHaveBeenCalled();
    expect(configStore.wasAppOpened("chat")).toBe(false);
  });

  it.each(["rewrite", "programs"] as AppId[])(
    "leaves %s to Settings, where its rows are",
    async (app) => {
      await switchAppFromSettings(app, true);
      expect(dueApps()).toEqual([]);
    },
  );

  it("opens several apps switched on in one visit, in one write", async () => {
    await switchAppFromSettings("tags", true);
    await switchAppFromSettings("connections", true);
    h.updateConfig.mockClear();
    openDueApps();
    expect(h.sidebarShow).toHaveBeenCalledTimes(1);
    expect(h.panelShow).toHaveBeenCalledTimes(1);
    expect(h.updateConfig).toHaveBeenCalledTimes(1);
    expect(h.updateConfig.mock.calls[0][0].apps.opened).toEqual(["tags", "connections"]);
  });
});
