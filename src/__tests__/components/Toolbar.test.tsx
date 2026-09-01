import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { Platform } from "../../lib/platform";

const h = vi.hoisted(() => ({
  platform: "mac" as "mac" | "win" | "linux",
  setSearchQuery: vi.fn(),
  formats: {
    bold: false,
    italic: false,
    code: false,
    bullet: false,
    task: false,
  },
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: true,
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      isOpen: () => true,
      searchQuery: () => "",
      setSearchQuery: h.setSearchQuery,
      searchHits: () => [],
      searchTotal: () => 0,
      searchMs: () => null,
    },
    tabs: {
      activeTabId: () => null,
      setActiveTabId: vi.fn(),
      closeTab: vi.fn(),
      restoreFromHistory: vi.fn(),
    },
    editor: { activeFormats: () => h.formats },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [], historyList: () => [], historyTotal: () => 0 },
}));
vi.mock("../../stores/global/workspace", () => ({ workspaceStore: { root: () => null } }));
vi.mock("../../stores/global/inbox", () => ({ inboxStore: { path: () => null } }));
vi.mock("../../components/Sidebar/ActiveSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/FilesSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/InboxSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/HistorySection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SearchResults", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SidebarEmpty", () => ({ default: () => null }));

import Toolbar from "../../components/Toolbar/Toolbar";
import Sidebar from "../../components/Sidebar/Sidebar";
import { registerCommand, unregisterCommand } from "../../commands/registry";

const FORMAT_CONTROLS = [
  ["editor.toggleBold", "Bold"],
  ["editor.toggleItalic", "Italic"],
  ["editor.toggleInlineCode", "Inline code"],
  ["editor.insertLink", "Insert link"],
  ["editor.toggleBulletList", "Bulleted list"],
  ["editor.toggleTaskList", "Task list"],
] as const;

const TOOLBAR_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Toolbar/Toolbar.css"),
  "utf8",
);

const registered: string[] = [];

function stub(id: string, label: string) {
  const execute = vi.fn();
  registerCommand({ id, label, scope: "editor", execute });
  registered.push(id);
  return execute;
}

/** The state of a markdown buffer: every formatting command is live. */
function withMarkdownBuffer(): Map<string, ReturnType<typeof vi.fn>> {
  const runs = new Map<string, ReturnType<typeof vi.fn>>();
  for (const [id, label] of FORMAT_CONTROLS) runs.set(id, stub(id, label));
  return runs;
}

function on(platform: Platform) {
  h.platform = platform;
}

function bar(container: HTMLElement): HTMLElement {
  return container.querySelector<HTMLElement>(".writ-toolbar")!;
}

function control(container: HTMLElement, name: string): HTMLButtonElement {
  return container.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!;
}

afterEach(() => {
  cleanup();
  for (const id of registered.splice(0)) unregisterCommand(id);
  h.platform = "mac";
  h.formats = { bold: false, italic: false, code: false, bullet: false, task: false };
  h.setSearchQuery.mockClear();
});

describe("Toolbar shape", () => {
  it("is a toolbar with a name", () => {
    const { container } = render(() => <Toolbar />);
    expect(bar(container).getAttribute("role")).toBe("toolbar");
    expect(bar(container).getAttribute("aria-label")).toBe("Note actions");
  });

  it("carries the sidebar toggle, New note, the formatting cluster and search", () => {
    const { container } = render(() => <Toolbar />);
    expect(control(container, "Toggle sidebar")).not.toBeNull();
    expect(container.querySelector(".writ-toolbar-compose")!.textContent).toContain("New note");
    const cluster = container.querySelectorAll(".writ-toolbar-cluster button");
    expect(Array.from(cluster).map((el) => el.getAttribute("aria-label"))).toEqual(
      FORMAT_CONTROLS.map(([, label]) => label),
    );
    expect(container.querySelector("input.search-input")).not.toBeNull();
  });

  it("names every icon-only control without a title attribute", () => {
    const { container } = render(() => <Toolbar />);
    for (const [, label] of FORMAT_CONTROLS) {
      expect(control(container, label), label).not.toBeNull();
    }
    expect(container.querySelector("[title]")).toBeNull();
  });

  it("switches to the Windows metrics on Windows", () => {
    on("win");
    const { container } = render(() => <Toolbar />);
    expect(bar(container).dataset.platform).toBe("win");
    expect(container.querySelector("input.search-input")).not.toBeNull();
    // jsdom loads no stylesheet, so the rule itself is what the test can read.
    expect(TOOLBAR_CSS).toMatch(/\.writ-toolbar\s*\{[^}]*height:\s*44px/);
    expect(TOOLBAR_CSS).toMatch(
      /\.writ-toolbar\[data-platform="win"\]\s*\{[^}]*height:\s*48px/,
    );
  });

  it("leaves search to the sidebar on Linux", () => {
    on("linux");
    const { container } = render(() => <Toolbar />);
    expect(bar(container).dataset.platform).toBe("linux");
    expect(container.querySelector("input.search-input")).toBeNull();
    expect(TOOLBAR_CSS).toMatch(
      /\.writ-toolbar\[data-platform="linux"\]\s*\{[^}]*gap:\s*var\(--writ-toolbar-tight, 6px\)[^}]*padding:\s*var\(--writ-toolbar-tight, 6px\)/,
    );
    expect(TOOLBAR_CSS).toMatch(/--writ-toolbar-tight:\s*6px/);
  });

  it("keeps New note on the baseline gap and out of the GNOME bold rule", () => {
    const compose = /\.writ-toolbar-compose\s*\{([^}]*)\}/.exec(TOOLBAR_CSS)![1];
    expect(compose).toMatch(/gap:\s*var\(--writ-toolbar-tight, 6px\)/);
    expect(compose).toMatch(/font-weight:\s*400/);
    expect(TOOLBAR_CSS).toMatch(
      /\[data-platform="win"\] \.writ-toolbar-compose\s*\{[^}]*gap:\s*var\(--writ-space-3\)/,
    );
  });
});

describe("Toolbar drag region", () => {
  it("makes the bar itself draggable on macOS, and no control", () => {
    const { container } = render(() => <Toolbar />);
    expect(bar(container).hasAttribute("data-tauri-drag-region")).toBe(true);
    for (const button of container.querySelectorAll("button")) {
      expect(button.hasAttribute("data-tauri-drag-region")).toBe(false);
    }
  });

  it("leaves the bar alone where the window keeps its own title bar", () => {
    on("win");
    const { container } = render(() => <Toolbar />);
    expect(bar(container).hasAttribute("data-tauri-drag-region")).toBe(false);
  });
});

describe("Toolbar commands", () => {
  it("runs sidebar.toggle from the toggle", () => {
    const run = stub("sidebar.toggle", "Toggle sidebar");
    const { container } = render(() => <Toolbar />);
    fireEvent.click(control(container, "Toggle sidebar"));
    expect(run).toHaveBeenCalledOnce();
  });

  it("runs note.new from New note", () => {
    const run = stub("note.new", "New note");
    const { container } = render(() => <Toolbar />);
    fireEvent.click(container.querySelector<HTMLButtonElement>(".writ-toolbar-compose")!);
    expect(run).toHaveBeenCalledOnce();
  });

  it("runs each formatting command from its own button", () => {
    const runs = withMarkdownBuffer();
    const { container } = render(() => <Toolbar />);
    for (const [id, label] of FORMAT_CONTROLS) {
      fireEvent.click(control(container, label));
      expect(runs.get(id), label).toHaveBeenCalledOnce();
    }
  });

  it("disables the formatting cluster while the note is not prose", () => {
    const { container } = render(() => <Toolbar />);
    for (const [, label] of FORMAT_CONTROLS) {
      expect(control(container, label).disabled, label).toBe(true);
    }
  });

  it("enables the cluster once a markdown buffer is active", () => {
    const { container } = render(() => <Toolbar />);
    expect(control(container, "Bold").disabled).toBe(true);
    withMarkdownBuffer();
    expect(control(container, "Bold").disabled).toBe(false);
  });
});

describe("Toolbar pressed state", () => {
  it("reports every toggle as off while the caret sits in plain prose", () => {
    withMarkdownBuffer();
    const { container } = render(() => <Toolbar />);
    for (const label of ["Bold", "Italic", "Inline code", "Bulleted list", "Task list"]) {
      expect(control(container, label).getAttribute("aria-pressed"), label).toBe("false");
    }
  });

  it("presses the controls for the constructs the caret is inside", () => {
    h.formats = { bold: true, italic: false, code: false, bullet: false, task: true };
    withMarkdownBuffer();
    const { container } = render(() => <Toolbar />);
    expect(control(container, "Bold").getAttribute("aria-pressed")).toBe("true");
    expect(control(container, "Task list").getAttribute("aria-pressed")).toBe("true");
    expect(control(container, "Italic").getAttribute("aria-pressed")).toBe("false");
    expect(control(container, "Bulleted list").getAttribute("aria-pressed")).toBe("false");
  });

  it("paints a pressed toggle over the bar's own ink and over the ghost hover", () => {
    h.formats = { bold: true, italic: false, code: false, bullet: false, task: false };
    withMarkdownBuffer();
    const { container } = render(() => <Toolbar />);
    const bold = control(container, "Bold");
    // The ink is decided by this exact class-and-attribute pair, so the rule
    // has to name the pair rather than leaning on Button.css.
    expect(bold.classList.contains("writ-toolbar-format")).toBe(true);
    expect(bold.getAttribute("aria-pressed")).toBe("true");

    const rules = TOOLBAR_CSS.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^}]*)\}/g);
    const pressed = [...rules].find(([, list]) => list.includes('[aria-pressed="true"]'));
    expect(pressed, "no pressed rule in Toolbar.css").toBeDefined();
    const [, selectors, body] = pressed!;
    const list = selectors.split(",").map((selector) => selector.trim());
    expect(list).toContain(
      ':root .writ-toolbar[data-platform] .writ-toolbar-format[aria-pressed="true"]',
    );
    // Hover on a pressed control must not fall back to the ghost hover.
    expect(list).toContain(
      ':root .writ-toolbar[data-platform] .writ-toolbar-format[aria-pressed="true"]:hover:not(:disabled)',
    );
    expect(body).toContain("background: var(--writ-bg-selected)");
    expect(body).toContain("color: var(--writ-fg)");
  });

  it("leaves Link unpressed: it inserts rather than toggles", () => {
    withMarkdownBuffer();
    const { container } = render(() => <Toolbar />);
    expect(control(container, "Insert link").hasAttribute("aria-pressed")).toBe(false);
  });
});

describe("Toolbar search", () => {
  it("hands the typed query to the search flow", () => {
    const { container } = render(() => <Toolbar />);
    const input = container.querySelector<HTMLInputElement>("input.search-input")!;
    fireEvent.input(input, { target: { value: "pricing" } });
    expect(h.setSearchQuery).toHaveBeenCalledWith("pricing");
  });

  it("is absent from the sidebar on macOS and Windows, and present on Linux", () => {
    const mac = render(() => <Sidebar />);
    expect(mac.container.querySelector(".search-bar")).toBeNull();
    cleanup();

    on("win");
    const win = render(() => <Sidebar />);
    expect(win.container.querySelector(".search-bar")).toBeNull();
    cleanup();

    on("linux");
    const linux = render(() => <Sidebar />);
    expect(linux.container.querySelector(".search-bar")).not.toBeNull();
  });
});

describe("Toolbar keyboard", () => {
  it("keeps one tab stop and moves it with the arrow keys", () => {
    withMarkdownBuffer();
    const { container } = render(() => <Toolbar />);
    const buttons = Array.from(container.querySelectorAll<HTMLButtonElement>("button"));
    expect(buttons.map((el) => el.tabIndex)).toEqual([0, ...buttons.slice(1).map(() => -1)]);

    fireEvent.keyDown(bar(container), { key: "ArrowRight" });
    expect(document.activeElement).toBe(buttons[1]);
    expect(buttons[1].tabIndex).toBe(0);
    expect(buttons[0].tabIndex).toBe(-1);

    fireEvent.keyDown(bar(container), { key: "ArrowLeft" });
    expect(document.activeElement).toBe(buttons[0]);
    expect(buttons[0].tabIndex).toBe(0);
  });

  it("leaves the arrow keys to the search field", () => {
    const { container } = render(() => <Toolbar />);
    const input = container.querySelector<HTMLInputElement>("input.search-input")!;
    input.focus();
    fireEvent.keyDown(input, { key: "ArrowRight" });
    expect(document.activeElement).toBe(input);
  });
});
