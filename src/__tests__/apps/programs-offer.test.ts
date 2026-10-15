import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot } from "solid-js";

// A program that calls while Connected programs is off is refused by the
// server, which writes its name down; Writ offers the app once, as one line
// with Turn on, under the same switch as the other offers.

const h = vi.hoisted(() => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  openSettings: vi.fn(),
}));

const calls = await vi.hoisted(async () => {
  const { createSignal: signal } = await import("solid-js");
  const [offCall, setOffCall] = signal<{ name: string; at: string } | null>(null);
  return { offCall, setOffCall };
});

vi.mock("../../services/tauri", () => ({ updateConfig: h.updateConfig, getConfig: h.getConfig }));
vi.mock("../../components/Activity/ActivityPanel", () => ({ openActivity: vi.fn() }));
vi.mock("../../components/SettingsModal/state", () => ({ openSettings: h.openSettings }));
vi.mock("../../stores/global/activity", () => ({
  activityStore: { offCall: calls.offCall },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => null },
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [] },
}));
vi.mock("../../stores/global/index-cache", () => ({
  indexCacheStore: {
    holdFacts: () => () => {},
    factsFor: () => () => undefined,
    factsSettledFor: () => () => false,
  },
}));

import { configStore } from "../../stores/global/config";
import { offerAppsInContext } from "../../commands/app-offers";
import { dismissToast, runToastAction, toasts } from "../../stores/global/notifications";

let dispose: (() => void) | undefined;

function watch(): void {
  createRoot((d) => {
    dispose = d;
    offerAppsInContext({ tabs: { activeTabId: () => null } as never });
  });
}

async function loadConfig(apps: Record<string, unknown>, mcpOn = false): Promise<void> {
  const fresh = configStore.config();
  h.getConfig.mockResolvedValue({
    ...fresh,
    mcp: { ...fresh.mcp, enabled: mcpOn },
    apps: { connections: false, graph: false, tags: false, offers: true, offered: [], opened: [], ...apps },
  });
  await configStore.load();
}

function lines(): string[] {
  return toasts().map((toast) => toast.text);
}

const CALL = { name: "claude-code", at: "2026-10-15T09:00:00Z" };

beforeEach(async () => {
  await loadConfig({});
  for (const toast of toasts()) dismissToast(toast.id);
  calls.setOffCall(null);
  vi.clearAllMocks();
});

afterEach(() => {
  dispose?.();
  for (const toast of toasts()) dismissToast(toast.id);
});

describe("the Connected programs offer", () => {
  it("offers the app once when a program called while it was off", () => {
    watch();
    expect(lines()).toEqual([]);

    calls.setOffCall(CALL);

    expect(lines()).toEqual(["Claude Code asked for your files, and Connected programs is off."]);
    expect(configStore.wasAppOffered("programs")).toBe(true);

    dismissToast(toasts()[0].id);
    calls.setOffCall({ name: "cursor-vscode", at: "2026-10-15T09:05:00Z" });
    expect(lines()).toEqual([]);
  });

  it("turns the app on and opens its rows, where the program waits", async () => {
    watch();
    calls.setOffCall(CALL);

    runToastAction(toasts()[0].id);

    await vi.waitFor(() => expect(h.openSettings).toHaveBeenCalledWith("apps", "mcp.waiting"));
    expect(configStore.isAppOn("programs")).toBe(true);
  });

  it("offers nothing while the app is on", async () => {
    await loadConfig({}, true);
    watch();
    calls.setOffCall(CALL);
    expect(lines()).toEqual([]);
  });

  it("offers nothing once it was offered", async () => {
    await loadConfig({ offered: ["programs"] });
    watch();
    calls.setOffCall(CALL);
    expect(lines()).toEqual([]);
  });

  it("offers nothing while offers are switched off", async () => {
    await loadConfig({ offers: false });
    watch();
    calls.setOffCall(CALL);
    expect(lines()).toEqual([]);
    expect(configStore.wasAppOffered("programs")).toBe(false);
  });
});
