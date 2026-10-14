import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot } from "solid-js";

// OD-5 (ADR-041 section 5): an app that is off is offered once, as one line
// with Turn on, when the open file shows a use for it. One switch turns every
// offer off.

interface Facts {
  links: unknown[];
  tags: unknown[];
  headings: unknown[];
  properties: unknown[];
}

const h = vi.hoisted(() => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  panelShow: vi.fn(),
  sidebarShow: vi.fn(),
  holds: [] as string[],
  releases: [] as string[],
}));

const files = await vi.hoisted(async () => {
  const { createSignal: signal } = await import("solid-js");
  const [facts, setFacts] = signal<Record<string, Facts>>({});
  const [active, setActive] = signal<{ id: string; source_path: string | null } | null>(null);
  return { facts, setFacts, active, setActive };
});

vi.mock("../../services/tauri", () => ({ updateConfig: h.updateConfig, getConfig: h.getConfig }));
vi.mock("../../components/Activity/ActivityPanel", () => ({ openActivity: vi.fn() }));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      rightPanel: { show: h.panelShow },
      sidebar: { show: h.sidebarShow },
    }),
  },
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => (files.active() ? [files.active()] : []) },
}));
vi.mock("../../stores/global/index-cache", () => ({
  indexCacheStore: {
    holdFacts: (path: string) => {
      h.holds.push(path);
      return () => h.releases.push(path);
    },
    factsFor: (path: string) => () => files.facts()[path],
    factsSettledFor: (path: string) => () => files.facts()[path] !== undefined,
  },
}));

import { configStore } from "../../stores/global/config";
import { offerAppsInContext } from "../../commands/app-offers";
import { dismissToast, runToastAction, toasts } from "../../stores/global/notifications";

const NONE: Facts = { links: [], tags: [], headings: [], properties: [] };
const LINKED: Facts = { ...NONE, links: [{ to_target: "B" }] };
const TAGGED: Facts = { ...NONE, tags: [{ name: "work" }] };

let dispose: (() => void) | undefined;

function watch(): void {
  createRoot((d) => {
    dispose = d;
    offerAppsInContext({ tabs: { activeTabId: () => files.active()?.id ?? null } as never });
  });
}

function open(path: string, facts?: Facts): void {
  if (facts) files.setFacts((held) => ({ ...held, [path]: facts }));
  files.setActive({ id: path, source_path: path });
}

function lines(): string[] {
  return toasts().map((toast) => toast.text);
}

beforeEach(async () => {
  const fresh = configStore.config();
  h.getConfig.mockResolvedValue({
    ...fresh,
    apps: { connections: false, graph: false, tags: false, offers: true, offered: [], opened: [] },
  });
  await configStore.load();
  for (const toast of toasts()) dismissToast(toast.id);
  files.setFacts({});
  files.setActive(null);
  h.holds = [];
  h.releases = [];
  vi.clearAllMocks();
});

afterEach(() => {
  dispose?.();
  for (const toast of toasts()) dismissToast(toast.id);
});

describe("an offer", () => {
  it("offers Tags once for a file with tags, and records it as it shows", () => {
    watch();
    open("/notes/a.md", TAGGED);
    expect(lines()).toEqual(["This file has tags. Tags lists them in the sidebar."]);
    expect(configStore.wasAppOffered("tags")).toBe(true);
    expect(h.updateConfig.mock.lastCall![0].apps.offered).toEqual(["tags"]);

    dismissToast(toasts()[0].id);
    open("/notes/b.md", TAGGED);
    expect(lines()).toEqual([]);
  });

  it("offers Connections for a file with links", () => {
    watch();
    open("/notes/a.md", LINKED);
    expect(lines()).toEqual([
      "This file links to other files. Connections lists its links beside it.",
    ]);
  });

  it("makes one offer at a time, and the other on a later file", () => {
    watch();
    open("/notes/a.md", { ...LINKED, tags: TAGGED.tags });
    expect(lines()).toHaveLength(1);
    dismissToast(toasts()[0].id);
    open("/notes/b.md", TAGGED);
    expect(lines()).toEqual(["This file has tags. Tags lists them in the sidebar."]);
  });

  it("says nothing for a file with neither, or one still being read", () => {
    watch();
    open("/notes/plain.txt", NONE);
    open("/notes/unread.md");
    expect(lines()).toEqual([]);
    expect(configStore.wasAppOffered("tags")).toBe(false);
  });

  it("turns the app on and shows it from Turn on", async () => {
    watch();
    open("/notes/a.md", TAGGED);
    runToastAction(toasts()[0].id);
    await vi.waitFor(() => expect(configStore.isAppOn("tags")).toBe(true));
    expect(h.sidebarShow).toHaveBeenCalledTimes(1);
    expect(configStore.wasAppOpened("tags")).toBe(true);
  });

  it("offers nothing for an app that is already on", async () => {
    await configStore.setAppOn("tags", true);
    watch();
    open("/notes/a.md", TAGGED);
    expect(lines()).toEqual([]);
  });
});

describe("before the config is read", () => {
  it("offers nothing and writes nothing", async () => {
    vi.resetModules();
    const fresh = await import("../../stores/global/config");
    const offers = await import("../../commands/app-offers");
    const notices = await import("../../stores/global/notifications");
    expect(fresh.configStore.loaded()).toBe(false);
    h.updateConfig.mockClear();
    let stop: (() => void) | undefined;
    createRoot((d) => {
      stop = d;
      offers.offerAppsInContext({
        tabs: { activeTabId: () => files.active()?.id ?? null } as never,
      });
    });
    open("/notes/a.md", TAGGED);
    expect(notices.toasts()).toEqual([]);
    expect(h.updateConfig).not.toHaveBeenCalled();
    expect(h.holds).toEqual([]);
    stop?.();
  });
});

describe("the switch for every offer", () => {
  it("offers nothing while it is off, and reads no file for it", () => {
    configStore.setAppOffersOn(false);
    watch();
    open("/notes/a.md", TAGGED);
    expect(lines()).toEqual([]);
    expect(h.holds).toEqual([]);
    expect(h.updateConfig.mock.lastCall![0].apps.offers).toBe(false);
  });

  it("stops reading the open file once every offer has been made", () => {
    watch();
    open("/notes/a.md", LINKED);
    dismissToast(toasts()[0].id);
    open("/notes/b.md", TAGGED);
    dismissToast(toasts()[0].id);
    const held = h.holds.length;
    open("/notes/c.md", TAGGED);
    expect(h.holds.length).toBe(held);
    expect(h.releases).toContain("/notes/b.md");
  });
});
