import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createRoot } from "solid-js";

// ADR-042 section 3: an app that is off has no menu item, no palette row and
// no sidebar section, and switching it on brings them back without a restart.

const h = vi.hoisted(() => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  chatHide: vi.fn(),
  graphClose: vi.fn(),
  selectTag: vi.fn(),
  closeActivity: vi.fn(),
  openSettings: vi.fn(),
  toggleChatPane: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({ updateConfig: h.updateConfig }));
vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => "win",
  detectPlatform: () => "win",
  IS_MAC: false,
  FILE_MANAGER_NAME: "Explorer",
  SHOW_IN_FILE_MANAGER: "Show in Explorer",
}));
vi.mock("../../components/Activity/ActivityPanel", () => ({ closeActivity: h.closeActivity }));
vi.mock("../../components/SettingsModal/SettingsModal", () => ({ openSettings: h.openSettings }));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      isOpen: () => true,
      searchQuery: () => "",
      setSearchQuery: vi.fn(),
      searchHits: () => [],
      searchTotal: () => 0,
      searchMs: () => null,
    },
    tabs: { activeTabId: () => null, setActiveTabId: vi.fn(), closeTab: vi.fn() },
    rightPanel: { isOpen: () => true },
    chatPanel: { isOpen: () => false },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [], historyList: () => [], historyTotal: () => 0 },
}));
vi.mock("../../stores/global/workspace", () => ({ workspaceStore: { root: () => "/notes" } }));
vi.mock("../../stores/global/inbox", () => ({ inboxStore: { files: () => [] } }));
vi.mock("../../components/Sidebar/FilesSection", () => ({
  default: () => <div data-testid="section-files" />,
}));
vi.mock("../../components/Sidebar/TagsSection", () => ({
  default: () => <div data-testid="section-tags" />,
}));
vi.mock("../../components/Sidebar/InboxSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/HistorySection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SearchResults", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SidebarEmpty", () => ({ default: () => null }));

import { configStore } from "../../stores/global/config";
import { registerCommand, unregisterCommand, getCommand } from "../../commands/registry";
import { defineAppCommands, resetAppCommands } from "../../commands/app-commands";
import { followAppSwitches } from "../../commands/app-switches";
import { createCommandProvider } from "../../commands/providers/command-provider";
import { appMenuItems } from "../../components/TitleBar/AppMenu";
import { MENU_COMMANDS } from "../../commands/menu-commands";
import { toggleChat } from "../../commands/chat";
import { REWRITE_COMMAND_IDS } from "../../commands/rewrite-actions";
import type { AppId } from "../../types/config";
import Sidebar from "../../components/Sidebar/Sidebar";
import Toolbar from "../../components/Toolbar/Toolbar";

// The labels App.tsx gives the app commands, which the Windows and Linux menu
// reads from the registry. Read from the file rather than copied, so a label
// App.tsx changes is the label this test looks for.
const APP_TSX = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");
function labelIn(id: string): string {
  const at = APP_TSX.indexOf(`id: "${id}"`);
  const label = /label: "([^"]+)"/.exec(APP_TSX.slice(at));
  if (at === -1 || !label) throw new Error(`${id} is not in App.tsx`);
  return label[1];
}

const APP_COMMANDS: Record<"connections" | "graph" | "programs", string> = {
  connections: "panel.toggle",
  graph: "folderGraph.open",
  programs: "activity.open",
};

let dispose: (() => void) | undefined;

function stand(): void {
  for (const [app, id] of Object.entries(APP_COMMANDS)) {
    defineAppCommands(app as AppId, [
      { id, label: labelIn(id), scope: "app", execute: () => {} },
    ]);
  }
  registerCommand({
    id: "chat.toggle",
    label: labelIn("chat.toggle"),
    keybinding: "CmdOrCtrl+Shift+A",
    scope: "app",
    global: true,
    app: "chat",
    execute: () => toggleChat(),
  });
  createRoot((d) => {
    dispose = d;
    followAppSwitches({
      chatPanel: { hide: h.chatHide } as never,
      folderGraph: { close: h.graphClose } as never,
      sidebar: { selectTag: h.selectTag } as never,
    });
  });
}

function paletteIds(): string[] {
  const provider = createCommandProvider({ listOnEmptyQuery: true });
  return provider
    .query("", new AbortController().signal, "commands")
    .map((row) => row.id.replace(/^command:/, ""));
}

function menuLabels(): string[] {
  return appMenuItems().map((item) => item.label);
}

async function turn(app: AppId, on: boolean): Promise<void> {
  await configStore.setAppOn(app, on);
}

beforeEach(async () => {
  await configStore.save({ ...configStore.config() });
  for (const app of ["chat", "rewrite", "programs", "connections", "graph", "tags"] as const) {
    await turn(app, false);
  }
  stand();
});

afterEach(() => {
  cleanup();
  dispose?.();
  resetAppCommands();
  unregisterCommand("chat.toggle");
  vi.clearAllMocks();
});

describe("Chat", () => {
  it("is out of the palette and the menu while off, and back once on", async () => {
    expect(paletteIds()).not.toContain("chat.toggle");
    expect(menuLabels()).not.toContain(labelIn("chat.toggle"));
    await turn("chat", true);
    expect(paletteIds()).toContain("chat.toggle");
    expect(menuLabels()).toContain(labelIn("chat.toggle"));
  });

  it("has a macOS menu item that belongs to it", () => {
    expect(MENU_COMMANDS.find((entry) => entry.id === "chat.toggle")?.app).toBe("chat");
  });

  it("answers its chord while off by opening its switch in Settings, Apps", () => {
    toggleChat();
    expect(h.openSettings).toHaveBeenCalledWith("apps", "ai.chat.enabled");
  });

  it("puts no button in the toolbar while off, and closes its pane", async () => {
    await turn("chat", true);
    const { queryByLabelText } = render(() => <Toolbar />);
    expect(queryByLabelText("Chat")).not.toBeNull();
    await turn("chat", false);
    expect(queryByLabelText("Chat")).toBeNull();
    expect(h.chatHide).toHaveBeenCalled();
  });
});

describe("Rewrite", () => {
  it("has no palette row while off, and its commands come with it", async () => {
    for (const id of REWRITE_COMMAND_IDS) expect(getCommand(id)).toBeUndefined();
    await turn("rewrite", true);
    for (const id of REWRITE_COMMAND_IDS) expect(paletteIds()).toContain(id);
    await turn("rewrite", false);
    for (const id of REWRITE_COMMAND_IDS) expect(paletteIds()).not.toContain(id);
  });

  it("has no menu item to take away", () => {
    for (const id of REWRITE_COMMAND_IDS) {
      expect(MENU_COMMANDS.some((entry) => entry.id === id)).toBe(false);
    }
  });
});

describe.each([
  ["connections", "panel.toggle"],
  ["graph", "folderGraph.open"],
  ["programs", "activity.open"],
] as const)("%s", (app, id) => {
  it("is not registered while off, so neither the palette nor the menu has it", async () => {
    expect(getCommand(id)).toBeUndefined();
    expect(paletteIds()).not.toContain(id);
    expect(menuLabels()).not.toContain(labelIn(id));
    await turn(app, true);
    expect(getCommand(id)).toBeDefined();
    expect(paletteIds()).toContain(id);
    if (MENU_COMMANDS.some((entry) => entry.id === id)) {
      expect(menuLabels()).toContain(labelIn(id));
    }
    await turn(app, false);
    expect(getCommand(id)).toBeUndefined();
  });
});

describe("Connections", () => {
  it("puts no button in the toolbar while off", async () => {
    const { queryByLabelText } = render(() => <Toolbar />);
    expect(queryByLabelText("Connections")).toBeNull();
    await turn("connections", true);
    expect(queryByLabelText("Connections")).not.toBeNull();
  });
});

describe("Graph", () => {
  it("closes the folder graph when switched off", async () => {
    await turn("graph", true);
    h.graphClose.mockClear();
    await turn("graph", false);
    expect(h.graphClose).toHaveBeenCalled();
  });
});

describe("Connected programs", () => {
  it("closes the Activity panel when switched off", async () => {
    await turn("programs", true);
    h.closeActivity.mockClear();
    await turn("programs", false);
    expect(h.closeActivity).toHaveBeenCalled();
  });
});

describe("Tags", () => {
  it("has no sidebar section while off, and gets it back once on", async () => {
    const { queryByTestId } = render(() => <Sidebar />);
    expect(queryByTestId("section-files")).not.toBeNull();
    expect(queryByTestId("section-tags")).toBeNull();
    await turn("tags", true);
    expect(queryByTestId("section-tags")).not.toBeNull();
  });

  it("clears the tag filter when switched off, so the tree is never left filtered", async () => {
    await turn("tags", true);
    h.selectTag.mockClear();
    await turn("tags", false);
    expect(h.selectTag).toHaveBeenCalledWith(null);
  });
});

describe("switching", () => {
  it("writes the config, which is what the menu bar is rebuilt from", async () => {
    h.updateConfig.mockClear();
    await turn("graph", true);
    expect(h.updateConfig).toHaveBeenCalledTimes(1);
    expect(h.updateConfig.mock.calls[0][0].apps.graph).toBe(true);
  });
});
