import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, beforeAll, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { Platform } from "../../lib/platform";

// The tab strip on the accepted baseline (ADR-030 §5): hidden at one note,
// borderless tabs, and the open one carrying the canvas colour rather than an
// accent-tinted pill.

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

// The strip reacts to the open set, so the fake registry hands it a signal
// rather than a plain array: a close has to take a tab off the screen.
const h = vi.hoisted(() => ({
  platform: "mac" as "mac" | "win" | "linux",
  tabs: (() => []) as () => Tab[],
  setTabs: ((_: Tab[]) => {}) as (tabs: Tab[]) => void,
  activeId: (() => null) as () => string | null,
  setActiveId: ((_: string | null) => {}) as (id: string | null) => void,
  closeTab: vi.fn(),
  setActiveTabId: vi.fn(),
  createTab: vi.fn(),
  renameBuffer: vi.fn(() => Promise.resolve()),
  focusEditor: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: true,
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    tabs: {
      activeTabId: () => h.activeId(),
      setActiveTabId: h.setActiveTabId,
      closeTab: h.closeTab,
      closeOtherTabs: vi.fn(),
      closeAllTabs: vi.fn(),
      createTab: h.createTab,
    },
    editor: { focusEditor: h.focusEditor, isRemovedOnDisk: () => false },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.tabs(),
    renameBuffer: h.renameBuffer,
  },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: () => h.activeId() } }) },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({ showContextMenu: vi.fn() }));
vi.mock("../../components/Notifications/Toast", () => ({ showToast: h.showToast }));

import TabBar from "../../components/Editor/TabBar";

const [tabsSignal, setTabsSignal] = createSignal<Tab[]>([]);
const [activeSignal, setActiveSignal] = createSignal<string | null>(null);
h.tabs = tabsSignal;
h.setTabs = setTabsSignal;
h.activeId = activeSignal;
h.setActiveId = setActiveSignal;

const TABBAR_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Editor/TabBar.css"),
  "utf8",
);

interface Rule {
  selectors: string[];
  declarations: Map<string, string>;
}

function rules(css: string): Rule[] {
  const parsed: Rule[] = [];
  for (const [, list, body] of css.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const declarations = new Map<string, string>();
    for (const line of body.split(";")) {
      const [property, ...rest] = line.split(":");
      if (rest.length === 0) continue;
      declarations.set(property.trim(), rest.join(":").trim());
    }
    parsed.push({ selectors: list.split(",").map((s) => s.trim()), declarations });
  }
  return parsed;
}

const RULES = rules(TABBAR_CSS);

function ruleFor(selector: string): Rule {
  const found = RULES.find((rule) => rule.selectors.includes(selector));
  if (!found) throw new Error(`no rule for ${selector}`);
  return found;
}

function open(count: number) {
  h.setTabs(
    Array.from({ length: count }, (_, i) => ({ id: `buf-${i + 1}`, title: `note-${i + 1}.md` })),
  );
  h.setActiveId(count > 0 ? "buf-1" : null);
}

/** Lets the close promise settle and Solid paint the strip that survives it. */
function settle(): Promise<void> {
  return new Promise((done) => setTimeout(done, 0));
}

function on(platform: Platform) {
  h.platform = platform;
}

function labels(container: HTMLElement): HTMLButtonElement[] {
  return Array.from(container.querySelectorAll<HTMLButtonElement>(".tab-label"));
}

afterEach(() => {
  cleanup();
  h.platform = "mac";
  h.setTabs([]);
  h.setActiveId(null);
  h.closeTab.mockClear();
  h.setActiveTabId.mockClear();
  h.createTab.mockClear();
  h.renameBuffer.mockClear();
  h.focusEditor.mockClear();
  h.showToast.mockClear();
});

describe("tab strip visibility", () => {
  it("is hidden with one open note", () => {
    open(1);
    const { container } = render(() => <TabBar />);
    expect(container.querySelector(".tabbar")).toBeNull();
  });

  it("is hidden with no open note", () => {
    open(0);
    const { container } = render(() => <TabBar />);
    expect(container.querySelector(".tabbar")).toBeNull();
  });

  it("is shown with two open notes", () => {
    open(2);
    const { container } = render(() => <TabBar />);
    expect(container.querySelector(".tabbar")).not.toBeNull();
    expect(labels(container).map((el) => el.textContent)).toEqual(["note-1.md", "note-2.md"]);
  });
});

describe("tab appearance", () => {
  it("seats the strip on the sidebar ground at the baseline metrics", () => {
    const bar = ruleFor(".tabbar");
    expect(bar.declarations.get("height")).toBe("36px");
    expect(bar.declarations.get("background")).toBe("var(--writ-bg-sidebar)");
    expect(bar.declarations.get("align-items")).toBe("flex-end");
    // The list scrolls under the tabs; the add control never scrolls away.
    expect(ruleFor(".tabbar-tabs").declarations.get("overflow-x")).toBe("auto");
    expect(bar.declarations.get("overflow-x")).toBeUndefined();
  });

  it("draws a borderless 28px tab with only its top corners rounded", () => {
    const tab = ruleFor(".tab");
    expect(tab.declarations.get("height")).toBe("28px");
    expect(tab.declarations.get("min-width")).toBe("100px");
    expect(tab.declarations.get("max-width")).toBe("200px");
    expect(tab.declarations.get("padding")).toBe("0 10px");
    expect(tab.declarations.get("border")).toBe("0");
    expect(tab.declarations.get("border-radius")).toBe(
      "var(--writ-r-tab) var(--writ-r-tab) 0 0",
    );
    expect(tab.declarations.get("background")).toBe("transparent");
    expect(tab.declarations.get("color")).toBe("var(--writ-fg-muted)");
  });

  it("gives the active tab the canvas colour", () => {
    const active = ruleFor(".tab-active");
    expect(active.declarations.get("background")).toBe("var(--writ-bg-canvas)");
    expect(active.declarations.get("color")).toBe("var(--writ-fg)");
    expect(active.declarations.get("font-weight")).toBe("500");
  });

  it("leaves no border, shadow or colour mix on any tab", () => {
    for (const rule of RULES) {
      const onATab = rule.selectors.some((s) => s.includes(".tab"));
      if (!onATab) continue;
      expect(rule.declarations.get("box-shadow"), rule.selectors.join(",")).toBeUndefined();
      const border = rule.declarations.get("border");
      if (border !== undefined) expect(border, rule.selectors.join(",")).toBe("0");
      for (const value of rule.declarations.values()) {
        expect(value).not.toContain("color-mix");
      }
    }
  });

  it("spends the accent on nothing but the GNOME tab indicator", () => {
    const accented = RULES.filter((rule) =>
      [...rule.declarations.values()].some((value) => value.includes("--writ-accent")),
    );
    expect(accented.map((rule) => rule.selectors.join(","))).toEqual([
      '.tabbar[data-platform="linux"] .tab-active::after',
    ]);
  });

  it("keeps the close control out of sight until the tab is reached", () => {
    expect(ruleFor(".tab-close").declarations.get("opacity")).toBe("0");
    const shown = RULES.find((rule) => rule.selectors.includes(".tab:hover .tab-close"))!;
    expect(shown.selectors).toEqual([
      ".tab:hover .tab-close",
      ".tab:focus-within .tab-close",
      ".tab-active .tab-close",
    ]);
    expect(shown.declarations.get("opacity")).toBe("1");
  });

  it("sizes the close glyph and the add button from the baseline", () => {
    const close = ruleFor(".tab-close");
    expect(close.declarations.get("width")).toBe("16px");
    expect(close.declarations.get("height")).toBe("16px");
    const add = ruleFor(".tab-add");
    expect(add.declarations.get("width")).toBe("24px");
    expect(add.declarations.get("height")).toBe("28px");

    open(2);
    const { container } = render(() => <TabBar />);
    const glyphs = container.querySelectorAll<SVGElement>(".tab-close .writ-icon");
    expect(glyphs.length).toBe(2);
    expect(glyphs[0].style.width).toBe("12px");
    expect(container.querySelector<SVGElement>(".tab-add .writ-icon")!.style.width).toBe("16px");
  });
});

describe("tab strip accessibility", () => {
  it("is a tablist of tabs, one of them selected", () => {
    open(2);
    const { container } = render(() => <TabBar />);
    const list = container.querySelector(".tabbar-tabs")!;
    expect(list.getAttribute("role")).toBe("tablist");
    expect(list.getAttribute("aria-label")).toBe("Open notes");
    const tabs = labels(container);
    expect(tabs.map((el) => el.getAttribute("role"))).toEqual(["tab", "tab"]);
    expect(tabs.map((el) => el.getAttribute("aria-selected"))).toEqual(["true", "false"]);
    expect(tabs.map((el) => el.tabIndex)).toEqual([0, -1]);
  });

  it("keeps the close control a sibling button, never one nested in the tab", () => {
    open(2);
    const { container } = render(() => <TabBar />);
    const tab = labels(container)[0];
    const close = container.querySelector<HTMLButtonElement>(".tab-close")!;
    expect(close.tagName).toBe("BUTTON");
    expect(close.parentElement).toBe(tab.parentElement);
    expect(tab.querySelector(".tab-close")).toBeNull();
    expect(close.getAttribute("aria-label")).toBe("Close note-1.md");
  });

  it("lets the tablist own its tabs and nothing else", () => {
    open(2);
    const { container } = render(() => <TabBar />);
    const list = container.querySelector<HTMLElement>('[role="tablist"]')!;

    // Only the tabs and their own close controls live under the list; the add
    // control is a sibling of it, not a stray child.
    const interactive = Array.from(
      list.querySelectorAll<HTMLElement>("button, input, select, textarea, a[href], [tabindex]"),
    );
    expect(interactive.length).toBeGreaterThan(0);
    for (const el of interactive) {
      expect(
        el.getAttribute("role") === "tab" || el.classList.contains("tab-close"),
        el.outerHTML,
      ).toBe(true);
    }
    const add = container.querySelector<HTMLElement>(".tab-add")!;
    expect(list.contains(add)).toBe(false);
    expect(container.querySelector(".tabbar")!.contains(add)).toBe(true);

    // Every tab reaches the list through presentational wrappers only, so the
    // list still owns it in the accessibility tree.
    for (const tab of labels(container)) {
      for (let el = tab.parentElement; el && el !== list; el = el.parentElement) {
        expect(["none", "presentation"], el.outerHTML).toContain(el.getAttribute("role"));
      }
    }
  });

  it("moves the selection with the arrow keys", () => {
    open(3);
    const { container } = render(() => <TabBar />);
    const tabs = labels(container);
    fireEvent.keyDown(tabs[0], { key: "ArrowRight" });
    expect(h.setActiveTabId).toHaveBeenCalledWith("buf-2");
    expect(document.activeElement).toBe(tabs[1]);

    fireEvent.keyDown(tabs[0], { key: "ArrowLeft" });
    expect(h.setActiveTabId).toHaveBeenLastCalledWith("buf-3");
    expect(document.activeElement).toBe(tabs[2]);
  });

  it("names every control without a title attribute", () => {
    open(2);
    const { container } = render(() => <TabBar />);
    expect(container.querySelector("[title]")).toBeNull();
    expect(container.querySelector(".tab-add")!.getAttribute("aria-label")).toBe("New note");
  });
});

describe("closing a tab from the keyboard", () => {
  it("closes on Enter and moves focus to the next tab", async () => {
    open(3);
    const { container } = render(() => <TabBar />);
    const closes = container.querySelectorAll<HTMLButtonElement>(".tab-close");
    h.closeTab.mockImplementation(async (id: string) => {
      h.setTabs(h.tabs().filter((tab) => tab.id !== id));
    });
    fireEvent.keyDown(closes[0], { key: "Enter" });
    await settle();
    expect(h.closeTab).toHaveBeenCalledWith("buf-1");
    expect(document.activeElement).toBe(labels(container)[0]);
    expect(labels(container)[0].textContent).toBe("note-2.md");
    expect(h.focusEditor).not.toHaveBeenCalled();
  });

  it("hands focus to the note when the close takes the strip away", async () => {
    open(2);
    const { container } = render(() => <TabBar />);
    const close = container.querySelector<HTMLButtonElement>(".tab-close")!;
    h.closeTab.mockImplementation(async (id: string) => {
      h.setTabs(h.tabs().filter((tab) => tab.id !== id));
    });
    fireEvent.keyDown(close, { key: " " });
    await settle();
    expect(h.closeTab).toHaveBeenCalledWith("buf-1");
    expect(container.querySelector(".tabbar")).toBeNull();
    expect(h.focusEditor).toHaveBeenCalledTimes(1);
  });
});

describe("platform layers", () => {
  it("marks the strip with the platform it renders for", () => {
    for (const platform of ["mac", "win", "linux"] as const) {
      on(platform);
      open(2);
      const { container, unmount } = render(() => <TabBar />);
      expect(container.querySelector(".tabbar")!.getAttribute("data-platform")).toBe(platform);
      unmount();
    }
  });

  it("takes the WinUI tab metrics on Windows", () => {
    const tab = ruleFor('.tabbar[data-platform="win"] .tab');
    expect(tab.declarations.get("min-height")).toBe("32px");
    expect(tab.declarations.get("max-width")).toBe("240px");
    expect(tab.declarations.get("padding")).toBe("8px 3px 4px 3px");
    expect(tab.declarations.get("font-size")).toBe("var(--writ-ui-sm)");
    const close = ruleFor('.tabbar[data-platform="win"] .tab-close');
    expect(close.declarations.get("width")).toBe("32px");
    expect(close.declarations.get("height")).toBe("24px");
  });

  it("takes the AdwTabBar chip metrics on Linux", () => {
    expect(ruleFor('.tabbar[data-platform="linux"]').declarations.get("height")).toBe("46px");
    const tab = ruleFor('.tabbar[data-platform="linux"] .tab');
    expect(tab.declarations.get("height")).toBe("26px");
    expect(tab.declarations.get("border-radius")).toBe("var(--writ-r-tab)");
    const active = ruleFor('.tabbar[data-platform="linux"] .tab-active');
    expect(active.declarations.get("background")).toBe("var(--writ-bg-selected)");
    const indicator = ruleFor('.tabbar[data-platform="linux"] .tab-active::after');
    expect(indicator.declarations.get("height")).toBe("2px");
    expect(indicator.declarations.get("background")).toBe("var(--writ-accent)");
    const close = ruleFor('.tabbar[data-platform="linux"] .tab-close');
    expect(close.declarations.get("width")).toBe("24px");
    expect(close.declarations.get("border-radius")).toBe("var(--writ-r-pill)");
  });
});

describe("renaming a tab", () => {
  function startRename(container: HTMLElement): HTMLInputElement {
    fireEvent.dblClick(container.querySelector<HTMLElement>(".tab-title")!);
    return container.querySelector<HTMLInputElement>(".tab-rename-input")!;
  }

  it("replaces the tab with the field rather than nesting one in the other", () => {
    open(2);
    const { container } = render(() => <TabBar />);
    const slot = container.querySelector<HTMLElement>(".tab")!;
    const input = startRename(container);
    expect(input.getAttribute("aria-label")).toBe("Rename note");
    expect(input.parentElement).toBe(slot);
    expect(input.closest("button")).toBeNull();
    // The tab it stands in for is gone while the field is up.
    expect(slot.querySelector('[role="tab"]')).toBeNull();
    expect(slot.querySelector(".tab-close")).not.toBeNull();
  });

  it("leaves the arrow keys to the caret while the field is up", () => {
    open(3);
    const { container } = render(() => <TabBar />);
    const input = startRename(container);
    fireEvent.keyDown(input, { key: "ArrowLeft" });
    expect(h.setActiveTabId).not.toHaveBeenCalled();
    expect(h.renameBuffer).not.toHaveBeenCalled();
    expect(container.querySelector(".tab-rename-input")).toBe(input);
  });

  it("opens the field on a double-click and commits on Enter", () => {
    open(2);
    const { container } = render(() => <TabBar />);
    const title = container.querySelector<HTMLElement>(".tab-title")!;
    const ev = new MouseEvent("dblclick", { bubbles: true, cancelable: true });
    const stopPropagation = vi.spyOn(ev, "stopPropagation");
    title.dispatchEvent(ev);

    const input = container.querySelector<HTMLInputElement>(".tab-rename-input")!;
    expect(input).not.toBeNull();
    expect(stopPropagation).toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", target: { value: "Pricing draft" } });
    expect(h.renameBuffer).toHaveBeenCalledWith("buf-1", "Pricing draft");
  });

  it("says so when the rename is refused", async () => {
    open(2);
    h.renameBuffer.mockImplementation(() => Promise.reject(new Error("refused")));
    const { container } = render(() => <TabBar />);
    const title = container.querySelector<HTMLElement>(".tab-title")!;
    fireEvent.dblClick(title);
    const input = container.querySelector<HTMLInputElement>(".tab-rename-input")!;
    fireEvent.keyDown(input, { key: "Enter", target: { value: "Pricing draft" } });
    await settle();
    // The message is the backend's own refusal, formatted by formatRenameError.
    expect(h.showToast).toHaveBeenCalledWith("refused", "error");
  });
});
