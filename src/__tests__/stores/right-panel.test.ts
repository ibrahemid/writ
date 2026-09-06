import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot } from "solid-js";

// The panel's own state: whether it shows, how wide it is, and which sections
// are folded. What a person set persists; a fold lasts the session.

const h = vi.hoisted(() => ({
  panel: { open: false, width: 240 },
  setPanelOpen: vi.fn<(open: boolean) => void>(),
  setPanelWidth: vi.fn<(width: number) => void>(),
}));

vi.mock("../../stores/global/config", async () => {
  const actual =
    await vi.importActual<typeof import("../../stores/global/config")>(
      "../../stores/global/config",
    );
  return {
    ...actual,
    configStore: {
      config: () => ({ panel: h.panel }),
      setPanelOpen: (open: boolean) => {
        h.panel = { ...h.panel, open };
        h.setPanelOpen(open);
      },
      setPanelWidth: (width: number) => {
        h.panel = { ...h.panel, width: actual.clampPanelWidth(width) };
        h.setPanelWidth(width);
      },
    },
  };
});

import { createRightPanelStore } from "../../stores/window/right-panel-store";
import { PANEL_WIDTH_MAX, PANEL_WIDTH_MIN } from "../../stores/global/config";

function withStore<T>(run: (store: ReturnType<typeof createRightPanelStore>) => T): T {
  return createRoot((dispose) => {
    const store = createRightPanelStore();
    const result = run(store);
    dispose();
    return result;
  });
}

beforeEach(() => {
  h.panel = { open: false, width: 240 };
  h.setPanelOpen.mockClear();
  h.setPanelWidth.mockClear();
});

describe("the panel beside the note", () => {
  it("starts closed on a first launch", () => {
    withStore((store) => {
      store.hydrateFromConfig();
      expect(store.isOpen()).toBe(false);
    });
  });

  it("opens where it was left", () => {
    h.panel = { open: true, width: 280 };
    withStore((store) => {
      store.hydrateFromConfig();
      expect(store.isOpen()).toBe(true);
      expect(store.width()).toBe(280);
    });
  });

  it("writes the open state through the config store on every flip", () => {
    withStore((store) => {
      store.toggle();
      expect(store.isOpen()).toBe(true);
      store.toggle();
      expect(store.isOpen()).toBe(false);
    });
    expect(h.setPanelOpen.mock.calls).toEqual([[true], [false]]);
  });

  it("keeps the width across a remount", () => {
    withStore((store) => store.setWidth(305));
    expect(h.setPanelWidth).toHaveBeenCalledWith(305);
    withStore((store) => {
      store.hydrateFromConfig();
      expect(store.width()).toBe(305);
    });
  });

  it("clamps the width to 200 and to 320", () => {
    withStore((store) => {
      store.setWidth(20);
      expect(store.width()).toBe(PANEL_WIDTH_MIN);
      store.setWidth(2000);
      expect(store.width()).toBe(PANEL_WIDTH_MAX);
    });
  });

  it("clamps a width a hand-edited config put out of range", () => {
    h.panel = { open: true, width: 900 };
    withStore((store) => expect(store.width()).toBe(PANEL_WIDTH_MAX));
  });

  it("folds a section and unfolds it, and writes nothing to disk for either", () => {
    withStore((store) => {
      expect(store.isCollapsed("outline")).toBe(false);
      store.toggleSection("outline");
      expect(store.isCollapsed("outline")).toBe(true);
      expect(store.isCollapsed("backlinks")).toBe(false);
      store.toggleSection("outline");
      expect(store.isCollapsed("outline")).toBe(false);
    });
    expect(h.setPanelOpen).not.toHaveBeenCalled();
    expect(h.setPanelWidth).not.toHaveBeenCalled();
  });

  it("show and hide are the two ends of the toggle", () => {
    withStore((store) => {
      store.show();
      expect(store.isOpen()).toBe(true);
      store.hide();
      expect(store.isOpen()).toBe(false);
    });
  });
});
