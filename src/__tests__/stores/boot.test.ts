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

import { createBootStore, createBootPlan, type BootPlan } from "../../stores/window/boot";
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
