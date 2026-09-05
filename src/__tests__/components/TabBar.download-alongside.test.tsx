import { describe, it, expect, vi, beforeAll, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { PendingDownload } from "../../stores/window/download-store";

// The strip carries two populations at once: notes with a buffer, and notes
// waiting on their bytes. Every other TabBar suite empties one of them, so the
// two together, and the hide-at-one-note rule counting a pending download, are
// what this one holds.

beforeAll(() => {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  if (!Element.prototype.scrollIntoView) {
    Element.prototype.scrollIntoView = vi.fn();
  }
});

interface Tab {
  id: string;
  title: string;
}

const h = vi.hoisted(() => ({
  tabs: (() => []) as () => Tab[],
  pending: (() => []) as () => PendingDownload[],
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => "mac",
  detectPlatform: () => "mac",
  IS_MAC: true,
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    tabs: {
      activeTabId: () => null,
      setActiveTabId: vi.fn(),
      closeTab: vi.fn(),
      closeOtherTabs: vi.fn(),
      closeAllTabs: vi.fn(),
      createTab: vi.fn(),
      newNote: vi.fn(),
    },
    editor: { focusEditor: vi.fn(), isRemovedOnDisk: () => false },
    downloads: {
      pending: () => h.pending(),
      selectedPath: () => null,
      select: vi.fn(),
      dismiss: vi.fn(),
    },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.tabs(),
    buffers: () => [],
    renameBuffer: vi.fn(() => Promise.resolve()),
  },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: () => null } }) },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({ showContextMenu: vi.fn() }));
vi.mock("../../components/Notifications/Toast", () => ({ showToast: vi.fn() }));

import TabBar from "../../components/Editor/TabBar";

const AWAY: PendingDownload = {
  path: "/home/user/Writ/away.md",
  title: "away.md",
  provider: "iCloud Drive",
  state: "downloading",
  reason: "download",
  message: null,
};

function mount(tabs: Tab[], pending: PendingDownload[]) {
  const [tabSignal] = createSignal(tabs);
  const [pendingSignal] = createSignal(pending);
  h.tabs = tabSignal;
  h.pending = pendingSignal;
  return render(() => <TabBar />);
}

describe("TabBar — a download beside notes that are here", () => {
  afterEach(() => {
    h.tabs = () => [];
    h.pending = () => [];
    cleanup();
  });

  it("shows both note tabs and the one waiting on its bytes", () => {
    const { container } = mount(
      [
        { id: "1", title: "first.md" },
        { id: "2", title: "second.md" },
      ],
      [AWAY],
    );

    const list = container.querySelector('[role="tablist"]')!;
    const tabs = list.querySelectorAll('[role="tab"]');
    expect(tabs.length).toBe(3);
    expect([...tabs].map((tab) => tab.querySelector(".tab-title")!.textContent)).toEqual([
      "first.md",
      "second.md",
      "away.md",
    ]);
    // The download is last and is the only one carrying a marker.
    expect(container.querySelectorAll(".tab-download").length).toBe(1);
    expect(container.querySelector(".tab-download-marker")!.textContent).toBe("downloading");
  });

  it("keeps the strip up at one note when a download is pending", () => {
    const { container } = mount([{ id: "1", title: "first.md" }], [AWAY]);

    expect(container.querySelector(".tabbar")).not.toBeNull();
    expect(container.querySelectorAll('[role="tab"]').length).toBe(2);
  });

  it("hides the strip at one note with nothing pending", () => {
    const { container } = mount([{ id: "1", title: "first.md" }], []);

    expect(container.querySelector(".tabbar")).toBeNull();
  });
});
