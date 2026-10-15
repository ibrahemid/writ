import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot } from "solid-js";

// The boot runs as named steps. Every listener and the keyboard handler are in
// place before Rust is told the frontend is ready and before the window shows,
// and a database that cannot list the open tabs shows the boot error screen on
// a window that is revealed rather than a dead one.

const h = vi.hoisted(() => ({
  showToast: vi.fn(),
  subscribed: [] as string[],
}));

vi.mock("../../stores/global/notifications", () => ({ showToast: h.showToast }));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async (kind: string) => {
    h.subscribed.push(kind);
    return () => {};
  }),
  emitFrontendReady: vi.fn(async () => {}),
}));

import {
  createBootStore,
  createBootPlan,
  openLinkDraft,
  openPendingPaths,
  type BootPlan,
} from "../../stores/window/boot";
import { windowRegistry } from "../../stores/global/window-registry";
import type { WindowState } from "../../stores/window/createWindowState";

type Log = string[];

function planRecording(log: Log, overrides: Partial<BootPlan> = {}): BootPlan {
  const step = (name: string) => async () => {
    log.push(name);
  };
  return {
    prepareShell: () => {
      log.push("shell");
      return [];
    },
    loadSettings: step("settings"),
    watchWindow: async () => {
      log.push("window");
      return [];
    },
    loadSession: step("session"),
    loadFolders: step("folders"),
    restoreRecovered: step("recovered"),
    registerCommands: () => void log.push("commands"),
    installKeyboard: () => {
      log.push("keyboard");
      return () => {};
    },
    subscriptions: [
      ["pending:opens", async () => (log.push("listen pending:opens"), () => {})],
      ["menu:action", async () => (log.push("listen menu:action"), () => {})],
      ["files:dropped", async () => (log.push("listen files:dropped"), () => {})],
    ],
    announceReady: step("ready"),
    armReveal: () => ({ now: () => void log.push("reveal") }),
    showFirstTab: step("first tab"),
    afterReveal: [["late", step("late")]],
    ...overrides,
  };
}

function boot() {
  return createRoot(() => createBootStore());
}

beforeEach(() => {
  h.showToast.mockReset();
  h.subscribed.length = 0;
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("the boot order", () => {
  it("listens and installs the keyboard handler before it reports ready and reveals", async () => {
    const log: Log = [];
    await boot().run(planRecording(log));

    const ready = log.indexOf("ready");
    const reveal = log.indexOf("reveal");
    for (const listener of log.filter((entry) => entry.startsWith("listen "))) {
      expect(log.indexOf(listener)).toBeLessThan(ready);
    }
    expect(log.indexOf("commands")).toBeLessThan(log.indexOf("listen menu:action"));
    expect(log.indexOf("keyboard")).toBeLessThan(reveal);
    expect(ready).toBeLessThan(reveal);
    expect(log.indexOf("late")).toBeGreaterThan(reveal);
  });

  it("goes on past a step that fails and still reveals", async () => {
    const log: Log = [];
    const store = boot();
    await store.run(
      planRecording(log, {
        loadFolders: async () => {
          throw new Error("folder gone");
        },
        showFirstTab: async () => {
          throw new Error("slow disk");
        },
      }),
    );
    expect(log).toContain("ready");
    expect(log).toContain("reveal");
    expect(log).toContain("late");
    expect(store.failure()).toBeNull();
  });

  it("subscribes every other event when one cannot be, and says so", async () => {
    const log: Log = [];
    await boot().run(
      planRecording(log, {
        subscriptions: [
          ["pending:opens", async () => (log.push("listen pending:opens"), () => {})],
          [
            "menu:action",
            async () => {
              throw new Error("no bridge");
            },
          ],
          ["files:dropped", async () => (log.push("listen files:dropped"), () => {})],
        ],
      }),
    );
    expect(log).toContain("listen files:dropped");
    expect(log).toContain("ready");
    expect(h.showToast).toHaveBeenCalledWith(expect.any(String), "error");
  });
});

describe("a database that cannot list the open tabs", () => {
  it("shows the boot error screen on a revealed window", async () => {
    const log: Log = [];
    const store = boot();
    await store.run(
      planRecording(log, {
        loadSession: async () => {
          throw new Error("database disk image is malformed");
        },
      }),
    );
    expect(store.failure()).toEqual({
      step: "session",
      detail: "database disk image is malformed",
    });
    expect(log).toContain("reveal");
    expect(log).not.toContain("ready");
    expect(log.some((entry) => entry.startsWith("listen "))).toBe(false);
  });
});

describe("the window's plan", () => {
  it("subscribes every event the window answers to", async () => {
    const win = { editor: {}, tabs: {} } as unknown as WindowState;
    const plan = createBootPlan(win, {
      prepareShell: () => [],
      registerCommands: () => {},
      installKeyboard: () => () => {},
      runMenuAction: () => {},
      afterReveal: [],
    });
    for (const [, subscribe] of plan.subscriptions) await subscribe();
    for (const kind of [
      "pending:opens",
      "files:dropped",
      "link:new",
      "context:asked",
      "review:asked",
      "menu:action",
      "buffer:external",
      "config:changed",
      "notes:swept",
      "workspace:changed",
      "workspace:swept",
      "inbox:file-arrived",
      "window:shown",
      "ai:rewrite",
      "ai:chat",
      "update:status",
      "hotkey:status",
      "hotkey:capture-status",
    ]) {
      expect(h.subscribed, kind).toContain(kind);
    }
  });
});

describe("files handed to the window", () => {
  it("opens each file at the line the writ command named for it", async () => {
    const reveals: Array<[string, number]> = [];
    const win = {
      windowId: 9101,
      tabs: {
        openPath: vi.fn(async (path: string) => ({
          kind: "opened",
          doc: { id: `id:${path}` },
          existed: false,
          mode: "Normal",
        })),
      },
      editor: {
        requestReveal: (id: string, line: number) => reveals.push([id, line]),
      },
    } as unknown as WindowState;
    const unregister = windowRegistry.register(win);
    windowRegistry.focus(9101);
    try {
      await openPendingPaths(["/n/a.md", "/n/b.md"], { "/n/b.md": 12 });
    } finally {
      unregister();
    }
    expect(reveals).toEqual([["id:/n/b.md", 12]]);
  });
});

describe("a new-tab link", () => {
  it("opens a tab called what the link named, and an untitled one without a name", async () => {
    const created: Array<string | undefined> = [];
    const win = {
      windowId: 9102,
      tabs: {
        createTab: vi.fn(async (title?: string) => {
          created.push(title);
          return { id: "draft" };
        }),
      },
      editor: {},
    } as unknown as WindowState;
    const unregister = windowRegistry.register(win);
    windowRegistry.focus(9102);
    try {
      await openLinkDraft("Launch notes");
      await openLinkDraft(null);
    } finally {
      unregister();
    }
    expect(created).toEqual(["Launch notes", undefined]);
  });

  it("says so when the tab could not be opened", async () => {
    h.showToast.mockClear();
    const win = {
      windowId: 9103,
      tabs: {
        createTab: vi.fn(async () => {
          throw new Error("title must not contain path separators");
        }),
      },
      editor: {},
    } as unknown as WindowState;
    const unregister = windowRegistry.register(win);
    windowRegistry.focus(9103);
    try {
      await openLinkDraft("x");
    } finally {
      unregister();
    }
    expect(h.showToast).toHaveBeenCalledWith("Could not open a new tab", "error");
  });
});
