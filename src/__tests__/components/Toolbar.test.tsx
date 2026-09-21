import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { Platform } from "../../lib/platform";

const h = vi.hoisted(() => ({
  platform: "mac" as "mac" | "win" | "linux",
  setSearchQuery: vi.fn(),
  panelOpen: false,
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
    rightPanel: { isOpen: () => h.panelOpen },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [], historyList: () => [], historyTotal: () => 0 },
}));
vi.mock("../../stores/global/workspace", () => ({ workspaceStore: { root: () => null } }));
vi.mock("../../stores/global/inbox", () => ({ inboxStore: { files: () => [] } }));
vi.mock("../../components/Sidebar/FilesSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/InboxSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/HistorySection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SearchResults", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SidebarEmpty", () => ({ default: () => null }));

import Toolbar from "../../components/Toolbar/Toolbar";
import Sidebar from "../../components/Sidebar/Sidebar";
import { registerCommand, unregisterCommand } from "../../commands/registry";

const FORMAT_COMMANDS = [
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

/** The state of a markdown buffer: every formatting command is registered. */
function withMarkdownBuffer(): void {
  for (const [id, label] of FORMAT_COMMANDS) stub(id, label);
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
  h.setSearchQuery.mockClear();
});

describe("Toolbar shape", () => {
  it("is a toolbar with a name", () => {
    const { container } = render(() => <Toolbar />);
    expect(bar(container).getAttribute("role")).toBe("toolbar");
    expect(bar(container).getAttribute("aria-label")).toBe("File actions");
  });

  it("carries the sidebar toggle, New note, connections and search", () => {
    const { container } = render(() => <Toolbar />);
    expect(control(container, "Toggle sidebar")).not.toBeNull();
    expect(container.querySelector(".writ-toolbar-compose")!.textContent).toContain("New file");
    expect(container.querySelector("input.search-input")).not.toBeNull();
    expect(control(container, "Connections")).not.toBeNull();
  });

  it("carries no formatting control, with the commands still on their keys", () => {
    withMarkdownBuffer();
    const { container } = render(() => <Toolbar />);
    for (const [, label] of FORMAT_COMMANDS) {
      expect(container.querySelector(`button[aria-label="${label}"]`), label).toBeNull();
    }
    expect(container.querySelector(".writ-toolbar-cluster")).toBeNull();
    expect(container.querySelector(".writ-toolbar-format")).toBeNull();
    expect(TOOLBAR_CSS).not.toContain("writ-toolbar-cluster");
    expect(TOOLBAR_CSS).not.toContain("writ-toolbar-format");
  });

  it("names every icon-only control without a title attribute", () => {
    const { container } = render(() => <Toolbar />);
    for (const label of ["Toggle sidebar", "Connections"]) {
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
      /\.writ-toolbar\[data-platform="linux"\]\s*\{[^}]*gap:\s*var\(--writ-toolbar-tight, var\(--writ-space-2-5\)\)[^}]*padding:\s*var\(--writ-toolbar-tight, var\(--writ-space-2-5\)\)/,
    );
    expect(TOOLBAR_CSS).toMatch(/--writ-toolbar-tight:\s*var\(--writ-space-2-5\)/);
  });

  it("keeps New note on the baseline gap and out of the GNOME bold rule", () => {
    const compose = /\.writ-toolbar-compose\s*\{([^}]*)\}/.exec(TOOLBAR_CSS)![1];
    expect(compose).toMatch(/gap:\s*var\(--writ-toolbar-tight, var\(--writ-space-2-5\)\)/);
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

  it("runs panel.toggle from the connections control, and shows its state", () => {
    const run = stub("panel.toggle", "Connections");
    const { container } = render(() => <Toolbar />);
    const button = control(container, "Connections");
    expect(button.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(button);
    expect(run).toHaveBeenCalledOnce();

    cleanup();
    h.panelOpen = true;
    const open = render(() => <Toolbar />);
    expect(control(open.container, "Connections").getAttribute("aria-pressed")).toBe("true");
    h.panelOpen = false;
  });

  it("runs note.new from New note", () => {
    const run = stub("note.new", "New file");
    const { container } = render(() => <Toolbar />);
    fireEvent.click(container.querySelector<HTMLButtonElement>(".writ-toolbar-compose")!);
    expect(run).toHaveBeenCalledOnce();
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
