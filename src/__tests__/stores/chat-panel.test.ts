import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot } from "solid-js";

// The chat column's own state: whether it shows and how wide it is. What a
// person set persists, and the range is the column's own — wider than the
// panel beside the note, because it holds a conversation and a proposal.

const h = vi.hoisted(() => ({
  chatPanel: { open: false, width: 380 },
  setChatPanelOpen: vi.fn<(open: boolean) => void>(),
  setChatPanelWidth: vi.fn<(width: number) => void>(),
}));

vi.mock("../../stores/global/config", async () => {
  const actual =
    await vi.importActual<typeof import("../../stores/global/config")>(
      "../../stores/global/config",
    );
  return {
    ...actual,
    configStore: {
      config: () => ({ chat_panel: h.chatPanel }),
      setChatPanelOpen: (open: boolean) => {
        h.chatPanel = { ...h.chatPanel, open };
        h.setChatPanelOpen(open);
      },
      setChatPanelWidth: (width: number) => {
        h.chatPanel = { ...h.chatPanel, width: actual.clampChatWidth(width) };
        h.setChatPanelWidth(width);
      },
    },
  };
});

import { createChatPanelStore } from "../../stores/window/chat-panel-store";
import {
  CHAT_WIDTH_DEFAULT,
  CHAT_WIDTH_MAX,
  CHAT_WIDTH_MIN,
  PANEL_WIDTH_MAX,
} from "../../stores/global/config";

function withStore<T>(run: (store: ReturnType<typeof createChatPanelStore>) => T): T {
  return createRoot((dispose) => {
    const store = createChatPanelStore();
    const result = run(store);
    dispose();
    return result;
  });
}

beforeEach(() => {
  h.chatPanel = { open: false, width: CHAT_WIDTH_DEFAULT };
  h.setChatPanelOpen.mockClear();
  h.setChatPanelWidth.mockClear();
});

describe("the chat column", () => {
  it("starts closed on a first launch", () => {
    withStore((store) => {
      store.hydrateFromConfig();
      expect(store.isOpen()).toBe(false);
    });
  });

  it("opens where it was left", () => {
    h.chatPanel = { open: true, width: 460 };
    withStore((store) => {
      store.hydrateFromConfig();
      expect(store.isOpen()).toBe(true);
      expect(store.width()).toBe(460);
    });
  });

  it("writes the open state through the config store on every flip", () => {
    withStore((store) => {
      store.toggle();
      expect(store.isOpen()).toBe(true);
      store.toggle();
      expect(store.isOpen()).toBe(false);
    });
    expect(h.setChatPanelOpen.mock.calls).toEqual([[true], [false]]);
  });

  it("keeps the width across a remount", () => {
    withStore((store) => store.setWidth(505));
    expect(h.setChatPanelWidth).toHaveBeenCalledWith(505);
    withStore((store) => {
      store.hydrateFromConfig();
      expect(store.width()).toBe(505);
    });
  });

  it("clamps the width to 320 and to 520", () => {
    withStore((store) => {
      store.setWidth(20);
      expect(store.width()).toBe(CHAT_WIDTH_MIN);
      store.setWidth(2000);
      expect(store.width()).toBe(CHAT_WIDTH_MAX);
    });
  });

  it("clamps a width a hand-edited config put out of range", () => {
    h.chatPanel = { open: true, width: 900 };
    withStore((store) => expect(store.width()).toBe(CHAT_WIDTH_MAX));
  });

  it("is wider than the panel beside the note", () => {
    expect(CHAT_WIDTH_MIN).toBeGreaterThanOrEqual(PANEL_WIDTH_MAX);
    expect(CHAT_WIDTH_DEFAULT).toBeGreaterThan(PANEL_WIDTH_MAX);
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
