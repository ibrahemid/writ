import { describe, it, expect, vi, afterEach, beforeAll, beforeEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
  isSidebarOpen: { value: false },
  showSidebar: vi.fn(() => {
    h.isSidebarOpen.value = true;
  }),
  hideSidebar: vi.fn(),
  recordCommandUse: vi.fn(),
  saveConfig: vi.fn().mockResolvedValue(undefined),
  paletteUsage: {} as Record<string, number>,
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: h.focusEditor },
    sidebar: {
      isOpen: () => h.isSidebarOpen.value,
      show: h.showSidebar,
      hide: h.hideSidebar,
      toggle: vi.fn(),
      searchQuery: () => "",
      setSearchQuery: vi.fn(),
      searchHits: () => [],
      searchTotal: () => 0,
      searchMs: () => null,
    },
    tabs: {
      activeTabId: () => null,
      setActiveTabId: vi.fn(),
      closeTab: vi.fn(),
      closeOtherTabs: vi.fn(),
      closeAllTabs: vi.fn(),
      createTab: vi.fn(),
      restoreFromHistory: vi.fn(),
      openFile: vi.fn(),
      openFileDialog: vi.fn(),
    },
  }),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ commands: { usage: h.paletteUsage }, theme: {}, keybindings: {} }),
    recordCommandUse: h.recordCommandUse,
    save: h.saveConfig,
  },
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => [],
    historyList: () => [],
    renameBuffer: vi.fn(),
    deleteFromHistory: vi.fn(),
    clearAllHistory: vi.fn(),
  },
}));

vi.mock("../../stores/global/theme", () => ({
  themeStore: {
    toConfig: () => ({}),
    loadConfig: vi.fn(),
    resolvedTokens: () => ({}),
    setOverride: vi.fn(),
    setPreset: vi.fn(),
    resetOverrides: vi.fn(),
    presetId: () => "default",
    presets: () => [{ id: "default", name: "Default" }],
    activePreset: () => ({}),
  },
}));

vi.mock("../../types/theme", () => ({
  TOKEN_GROUPS: [],
}));

vi.mock("../Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import CommandPalette, {
  openCommandPalette,
  closeCommandPalette,
} from "../../components/CommandPalette/CommandPalette";
import ThemeEditor, {
  openThemeEditor,
  closeThemeEditor,
} from "../../components/ThemeEditor/ThemeEditor";
import ShortcutEditor, {
  openShortcutEditor,
  closeShortcutEditor,
} from "../../components/ShortcutEditor/ShortcutEditor";
import {
  installKeyboardHandler,
  uninstallKeyboardHandler,
  rebuildKeyMap,
} from "../../commands/keybindings";
import {
  registerCommand,
  getAllCommands,
  unregisterCommand,
} from "../../commands/registry";
import {
  resetModalStack,
  isModalOpen,
  modalOpenCount,
  pushModal,
  popModal,
} from "../../lib/modal-stack";

const probe = vi.fn();

function resetDom() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
}

function mountAppShell() {
  resetDom();
  const appRoot = document.createElement("div");
  appRoot.id = "app";
  document.body.appendChild(appRoot);
  const container = document.createElement("div");
  container.className = "app-container";
  appRoot.appendChild(container);
  return container;
}

async function flush() {
  await new Promise<void>((r) => setTimeout(r, 0));
}

describe("Modal background suppression", () => {
  beforeEach(() => {
    for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
    probe.mockClear();
    registerCommand({
      id: "test.probe",
      label: "Test Probe",
      scope: "app",
      keybinding: "CmdOrCtrl+1",
      execute: probe,
    });
    rebuildKeyMap();
    installKeyboardHandler();
    h.showSidebar.mockClear();
    h.hideSidebar.mockClear();
    h.focusEditor.mockClear();
    h.recordCommandUse.mockClear();
    h.isSidebarOpen.value = false;
    resetModalStack();
  });

  afterEach(() => {
    uninstallKeyboardHandler();
    closeCommandPalette();
    closeThemeEditor();
    closeShortcutEditor();
    cleanup();
    resetDom();
    resetModalStack();
  });

  it("palette open blocks a global command keybinding", async () => {
    const container = mountAppShell();
    const user = userEvent.setup({ document });
    render(() => <CommandPalette />, { container });
    openCommandPalette();
    await flush();
    expect(isModalOpen()).toBe(true);
    await user.keyboard("{Meta>}1{/Meta}");
    expect(probe).not.toHaveBeenCalled();
  });

  it("palette open keeps focus inside the palette", async () => {
    const container = mountAppShell();
    render(() => <CommandPalette />, { container });
    openCommandPalette();
    await flush();
    const palette = document.querySelector(".palette");
    expect(palette).not.toBeNull();
    expect(palette!.contains(document.activeElement)).toBe(true);
  });

  it("palette open does not let arrow keys escape the input", async () => {
    const container = mountAppShell();
    const user = userEvent.setup({ document });
    render(() => <CommandPalette />, { container });
    openCommandPalette();
    await flush();
    const paletteInput = document.querySelector<HTMLInputElement>(".palette-input")!;
    expect(document.activeElement).toBe(paletteInput);
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).toBe(paletteInput);
  });

  it("theme editor open blocks a global command keybinding", async () => {
    const container = mountAppShell();
    const user = userEvent.setup({ document });
    render(() => <ThemeEditor />, { container });
    openThemeEditor();
    await flush();
    expect(isModalOpen()).toBe(true);
    await user.keyboard("{Meta>}1{/Meta}");
    expect(probe).not.toHaveBeenCalled();
  });

  it("shortcut editor open blocks a global command keybinding", async () => {
    const container = mountAppShell();
    const user = userEvent.setup({ document });
    render(() => <ShortcutEditor />, { container });
    openShortcutEditor();
    await flush();
    expect(isModalOpen()).toBe(true);
    await user.keyboard("{Meta>}1{/Meta}");
    expect(probe).not.toHaveBeenCalled();
  });

  it("palette close restores global handlers", async () => {
    const container = mountAppShell();
    const user = userEvent.setup({ document });
    render(() => <CommandPalette />, { container });
    openCommandPalette();
    await flush();
    expect(isModalOpen()).toBe(true);
    await user.keyboard("{Meta>}1{/Meta}");
    expect(probe).not.toHaveBeenCalled();

    closeCommandPalette();
    await flush();
    expect(isModalOpen()).toBe(false);

    document.body.focus();
    await user.keyboard("{Meta>}1{/Meta}");
    expect(probe).toHaveBeenCalled();
  });

  it("nested modals keep suppression until all closed", async () => {
    pushModal();
    pushModal();
    expect(modalOpenCount()).toBe(2);
    expect(isModalOpen()).toBe(true);
    popModal();
    expect(isModalOpen()).toBe(true);
    popModal();
    expect(isModalOpen()).toBe(false);
  });
});
