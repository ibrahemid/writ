import { describe, it, expect, vi, beforeAll, afterEach, onTestFinished } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

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
  Element.prototype.scrollIntoView = vi.fn();
});

interface Tab {
  id: string;
  title: string;
  source_path: string | null;
}

const h = vi.hoisted(() => ({
  tabs: (() => []) as () => Tab[],
  history: [] as { id: string }[],
  activeId: (() => null) as () => string | null,
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  showContextMenu: vi.fn(),
  showAnchoredMenu: vi.fn(),
  copyText: vi.fn(() => Promise.resolve()),
  states: new Map<string, string>(),
  executeCommand: vi.fn(),
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => "mac",
  detectPlatform: () => "mac",
  IS_MAC: true,
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    panes: {
      holds: () => true,
      activeIn: () => (() => h.activeId())(),
      isSplit: () => false,
      focused: () => "left",
      focus: () => {},
    },
    tabs: {
      selectIn: (_pane: string, id: string) => h.setActiveTabId(id),
      showDownloadPane: () => h.setActiveTabId(null),
      moveToOtherPane: () => {},
      activeTabId: () => h.activeId(),
      setActiveTabId: h.setActiveTabId,
      closeTab: h.closeTab,
      closeOtherTabs: vi.fn(),
      closeAllTabs: vi.fn(),
      newNote: vi.fn(),
    },
    editor: { focusEditor: vi.fn(), isRemovedOnDisk: () => false, tierOf: () => null },
    downloads: {
      pending: () => [],
      selectedPath: () => null,
      select: () => {},
      dismiss: async () => {},
    },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.tabs(),
    buffers: () => h.tabs(),
    historyList: () => h.history,
  },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: () => h.activeId() } }) },
}));
vi.mock("../../stores/global/notes", () => ({
  notesStore: { root: () => "/Users/me/Writ", contains: () => true },
}));
vi.mock("../../stores/global/workspace", () => ({ workspaceStore: { root: () => null } }));
vi.mock("../../stores/global/clipboard", () => ({ clipboardStore: { copyText: h.copyText } }));
vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { stateOf: (id: string) => h.states.get(id) ?? "clean" },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: h.showContextMenu,
  showAnchoredMenu: h.showAnchoredMenu,
  isMenuOpenFor: () => false,
}));
vi.mock("../../commands/registry", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  executeCommand: h.executeCommand,
}));

import TabBar from "../../components/Editor/TabBar";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import { formatKeybinding } from "../../lib/keybinding-format";

const [tabsSignal, setTabsSignal] = createSignal<Tab[]>([]);
const [activeSignal, setActiveSignal] = createSignal<string | null>(null);
h.tabs = tabsSignal;
h.activeId = activeSignal;

function open(tabs: Tab[], active = tabs[0]?.id ?? null) {
  setTabsSignal(tabs);
  setActiveSignal(active);
}

function tab(id: string, path: string): Tab {
  return { id, title: path.split("/").pop()!, source_path: path };
}

function list(container: HTMLElement): HTMLElement {
  return container.querySelector<HTMLElement>('[role="tablist"]')!;
}

function overflowing(el: HTMLElement, scrollWidth: number, clientWidth: number) {
  Object.defineProperty(el, "scrollWidth", { configurable: true, value: scrollWidth });
  Object.defineProperty(el, "clientWidth", { configurable: true, value: clientWidth });
}

afterEach(() => {
  cleanup();
  h.history = [];
  h.states.clear();
  vi.clearAllMocks();
});

describe("tabs that share a name", () => {
  it("show the shortest folder that tells them apart", () => {
    open([
      tab("a", "/Users/me/Writ/api/README.md"),
      tab("b", "/Users/me/Writ/web/README.md"),
      tab("c", "/Users/me/Writ/plan.md"),
    ]);
    const { container } = render(() => <TabBar />);
    const tabs = [...container.querySelectorAll<HTMLButtonElement>('[role="tab"]')];

    expect(tabs.map((el) => el.querySelector(".tab-folder")?.textContent ?? null)).toEqual([
      "api",
      "web",
      null,
    ]);
    expect(tabs[0].getAttribute("aria-label")).toBe("README.md in api");
    expect(tabs[2].hasAttribute("aria-label")).toBe(false);
  });

  it("give the path inside the notes folder in the tooltip", async () => {
    vi.useFakeTimers();
    open([tab("a", "/Users/me/Writ/api/README.md"), tab("b", "/Users/me/Writ/web/README.md")]);
    const { container } = render(() => <TabBar />);
    const anchor = container.querySelector<HTMLElement>(".tab .writ-tooltip-anchor")!;
    fireEvent.pointerEnter(anchor);
    await vi.advanceTimersByTimeAsync(600);
    expect(document.querySelector('[role="tooltip"]')!.textContent).toBe("api/README.md");
    vi.useRealTimers();
  });
});

describe("a strip wider than the window", () => {
  const twelve = Array.from({ length: 12 }, (_, i) => tab(`t${i}`, `/Users/me/Writ/file-${i}.md`));

  it("shows no overflow control while every tab fits", () => {
    open(twelve);
    const { container } = render(() => <TabBar />);
    expect(container.querySelector(".tab-overflow")).toBeNull();
  });

  it("fades the clipped edge and offers every open file from the overflow menu", async () => {
    open(twelve);
    h.states.set("t3", "dirty");
    h.states.set("t7", "failed");
    const { container } = render(() => <TabBar />);
    const strip = list(container);
    overflowing(strip, 2400, 1000);
    fireEvent.scroll(strip);

    expect(strip.classList.contains("is-clipped-end")).toBe(true);
    expect(strip.classList.contains("is-clipped-start")).toBe(false);
    const button = container.querySelector<HTMLButtonElement>(".tab-overflow")!;
    button.click();

    const items = h.showAnchoredMenu.mock.calls[0][1] as {
      label: string;
      checked?: boolean;
      marker?: string;
      action: () => void;
    }[];
    expect(items.map((item) => item.label)).toEqual(twelve.map((_, i) => `file-${i}`));
    expect(items[0].checked).toBe(true);
    expect(items[3].marker).toBe("unsaved");
    expect(items[7].marker).toBe("failed");
    expect(items[1].marker).toBeUndefined();

    items[11].action();
    expect(h.setActiveTabId).toHaveBeenCalledWith("t11");
  });

  it("scrolls sideways under a vertical wheel", () => {
    open(twelve);
    const { container } = render(() => <TabBar />);
    const strip = list(container);
    overflowing(strip, 2400, 1000);

    const event = new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true });
    strip.dispatchEvent(event);

    expect(strip.scrollLeft).toBe(120);
    expect(event.defaultPrevented).toBe(true);
    expect(strip.classList.contains("is-clipped-start")).toBe(true);
  });

  it("leaves a sideways wheel to the browser", () => {
    open(twelve);
    const { container } = render(() => <TabBar />);
    const strip = list(container);
    overflowing(strip, 2400, 1000);

    const event = new WheelEvent("wheel", { deltaX: 80, deltaY: 10, bubbles: true, cancelable: true });
    strip.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });

  it("brings the tab the arrow keys select into view", () => {
    const scrolled: string[] = [];
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = function (this: Element) {
      scrolled.push(this.textContent ?? "");
    };
    onTestFinished(() => {
      Element.prototype.scrollIntoView = original;
    });
    open(twelve);
    const { container } = render(() => <TabBar />);
    h.setActiveTabId.mockImplementation((id: string) => setActiveSignal(id));
    const first = container.querySelector<HTMLButtonElement>('[role="tab"]')!;

    fireEvent.keyDown(first, { key: "ArrowLeft" });

    expect(h.setActiveTabId).toHaveBeenCalledWith("t11");
    expect(scrolled[scrolled.length - 1]).toBe("file-11");
  });
});

describe("the tab menu", () => {
  function menuFor(container: HTMLElement, index = 0) {
    const tabs = container.querySelectorAll<HTMLElement>(".tab");
    fireEvent.contextMenu(tabs[index], { clientX: 1, clientY: 1 });
    const calls = h.showContextMenu.mock.calls;
    return calls[calls.length - 1][2] as {
      label: string;
      kbd?: string;
      disabled?: boolean;
      action: () => void;
    }[];
  }

  it("names the chords for Rename, Close tab and Reopen closed tab", () => {
    registerCommand({ id: "note.rename", label: "Rename file…", keybinding: "F2", scope: "app", execute: () => {} });
    registerCommand({ id: "buffer.close", label: "Close tab", keybinding: "CmdOrCtrl+W", scope: "app", execute: () => {} });
    registerCommand({
      id: "history.restoreLast",
      label: "Reopen closed tab",
      keybinding: "CmdOrCtrl+Shift+T",
      scope: "app",
      execute: () => {},
    });
    open([tab("a", "/Users/me/Writ/a.md")]);
    const { container } = render(() => <TabBar />);
    const items = menuFor(container);
    const kbd = (label: string) => items.find((item) => item.label === label)?.kbd;

    expect(kbd("Rename")).toBe("F2");
    expect(kbd("Close tab")).toBe(formatKeybinding("CmdOrCtrl+W"));
    expect(kbd("Reopen closed tab")).toBe(formatKeybinding("CmdOrCtrl+Shift+T"));
    unregisterCommand("note.rename");
    unregisterCommand("buffer.close");
    unregisterCommand("history.restoreLast");
  });

  it("copies the file's path", () => {
    open([tab("a", "/Users/me/Writ/a.md")]);
    const { container } = render(() => <TabBar />);
    const copy = menuFor(container).find((item) => item.label === "Copy path")!;
    expect(copy.disabled).toBe(false);
    copy.action();
    expect(h.copyText).toHaveBeenCalledWith("/Users/me/Writ/a.md");
  });

  it("has no path to copy for a file not written yet", () => {
    open([{ id: "a", title: "untitled.md", source_path: null }]);
    const { container } = render(() => <TabBar />);
    expect(menuFor(container).find((item) => item.label === "Copy path")!.disabled).toBe(true);
  });

  it("reopens the last closed tab, and only when one was closed", () => {
    open([tab("a", "/Users/me/Writ/a.md")]);
    const { container } = render(() => <TabBar />);
    expect(menuFor(container).find((item) => item.label === "Reopen closed tab")!.disabled).toBe(true);

    h.history = [{ id: "old" }];
    const reopen = menuFor(container).find((item) => item.label === "Reopen closed tab")!;
    expect(reopen.disabled).toBe(false);
    reopen.action();
    expect(h.executeCommand).toHaveBeenCalledWith("history.restoreLast");
  });
});
