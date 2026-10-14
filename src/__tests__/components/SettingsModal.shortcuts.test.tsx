import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";

// The nav drops a section no row on this platform can fill, and Files holds
// only the file-types row, which macOS alone can claim. jsdom reports no
// platform, so name one before the module graph reads it.
vi.hoisted(() => {
  Object.defineProperty(globalThis.navigator, "platform", {
    value: "MacIntel",
    configurable: true,
  });
});

const mocks = vi.hoisted(() => ({
  accentApplies: vi.fn(() => true),
  activePresetId: vi.fn(() => "warp-dark"),
  polarity: vi.fn(() => "light"),
  setPreset: vi.fn(),
  focusEditor: vi.fn(),
  openThemeEditor: vi.fn(),
  fetchDefaultAppStatus: vi.fn().mockResolvedValue({ status: "unsupported" }),
  claimDefaultApp: vi.fn().mockResolvedValue(undefined),
  fetchDefaultAppTypes: vi.fn(),
  fetchCliStatus: vi.fn().mockResolvedValue({ installed: false }),
  fetchStorageInfo: vi.fn().mockResolvedValue({ db_path: "/home/user/.writ/writ.db", dir: "/home/user/.writ" }),
  revealStoragePath: vi.fn().mockResolvedValue(undefined),
  copyStoragePath: vi.fn().mockResolvedValue(undefined),
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
  openThirdPartyNoticesBuffer: vi.fn(),
  setActiveTabId: vi.fn(),
  requestExternalReload: vi.fn(),
  aiEndpointState: vi.fn(),
  aiConsentHost: vi.fn(),
  notesFolder: vi.fn(() => null),
  notesLoadFolder: vi.fn().mockResolvedValue(undefined),
  notesShowInFileManager: vi.fn(),
  notesCopyPath: vi.fn(),
  notesMove: vi.fn(),
  activityRecent: vi.fn().mockResolvedValue([]),
  activityClear: vi.fn().mockResolvedValue(undefined),
  mcpClients: vi.fn().mockResolvedValue({ approved: [], waiting: [] }),
  mcpSetClientPermission: vi.fn(),
  mcpForgetClient: vi.fn(),
  mcpServerCommand: vi.fn().mockResolvedValue({ path: "/usr/local/bin/writ", command: "writ mcp" }),
  mcpTools: vi.fn(),
  onEvent: vi.fn().mockResolvedValue(() => {}),
  aiCheckConnection: vi.fn().mockResolvedValue({
    reachable: true,
    model_listed: true,
    kind: "ok",
    detail: "",
    models: [],
  }),
}));

const chord = vi.hoisted(() => ({
  setTaken: (_taken: boolean) => {},
  setUnsupported: (_unsupported: boolean) => {},
}));

const login = vi.hoisted(() => ({
  setEnabled: (_enabled: boolean) => {},
  load: vi.fn().mockResolvedValue(undefined),
  set: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  writeClipboardText: mocks.writeClipboardText,
  getConfig: vi.fn().mockResolvedValue(undefined),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  searchBuffers: vi.fn().mockResolvedValue([]),
  aiHasApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
  aiSetApiKey: vi.fn().mockResolvedValue({ is_set: true, memory_only: false }),
  aiClearApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
  aiEndpointState: mocks.aiEndpointState,
  aiConsentHost: mocks.aiConsentHost,
  aiCheckConnection: mocks.aiCheckConnection,
  aiProviders: vi.fn().mockResolvedValue([]),
  aiListModels: vi.fn().mockResolvedValue({ models: [] }),
  aiProbeLocal: vi.fn().mockResolvedValue({ ollama: false, lmstudio: false }),
  aiOpenrouterConnect: vi.fn().mockResolvedValue({ is_set: true, memory_only: false }),
  aiOpenrouterCancel: vi.fn().mockResolvedValue(undefined),
  openExternalUrl: vi.fn().mockResolvedValue(undefined),
  classifyExternalUrl: vi.fn().mockResolvedValue({ kind: "allow" }),
  activityRecent: mocks.activityRecent,
  activityClear: mocks.activityClear,
  mcpClients: mocks.mcpClients,
  mcpSetClientPermission: mocks.mcpSetClientPermission,
  mcpForgetClient: mocks.mcpForgetClient,
  mcpServerCommand: mocks.mcpServerCommand,
  mcpTools: mocks.mcpTools,
}));

vi.mock("../../services/events", () => ({
  onEvent: mocks.onEvent,
  emitFrontendReady: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../stores/global/default-app", () => ({
  fetchDefaultAppStatus: mocks.fetchDefaultAppStatus,
  claimDefaultApp: mocks.claimDefaultApp,
  fetchDefaultAppTypes: mocks.fetchDefaultAppTypes,
}));

vi.mock("../../stores/global/cli", () => ({
  installCli: vi.fn().mockResolvedValue({ symlink_path: "/usr/local/bin/writ", manual_command: "" }),
  fetchCliStatus: mocks.fetchCliStatus,
  CLI_EXAMPLES: ["writ .", "git log | writ --title log"],
  copyCliExample: vi.fn().mockResolvedValue(undefined),
  installFailureMessage: () => "Could not install the writ command",
}));

vi.mock("../../stores/global/notes", () => ({
  notesStore: {
    folder: mocks.notesFolder,
    loadFolder: mocks.notesLoadFolder,
    showInFileManager: mocks.notesShowInFileManager,
    copyPath: mocks.notesCopyPath,
    move: mocks.notesMove,
  },
}));

vi.mock("../../stores/global/storage", () => ({
  fetchStorageInfo: mocks.fetchStorageInfo,
  revealStoragePath: mocks.revealStoragePath,
  copyStoragePath: mocks.copyStoragePath,
}));


vi.mock("../../stores/global/notices", () => ({
  THIRD_PARTY_NOTICES_TITLE: "Third-party licences",
  openThirdPartyNoticesBuffer: mocks.openThirdPartyNoticesBuffer,
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: mocks.focusEditor, requestExternalReload: mocks.requestExternalReload },
    tabs: { setActiveTabId: mocks.setActiveTabId },
  }),
  default: (props: { children: unknown }) => props.children,
}));

vi.mock("../../components/ThemeEditor/ThemeEditor", () => ({
  openThemeEditor: mocks.openThemeEditor,
  default: () => null,
}));

vi.mock("../../components/ShortcutEditor/ShortcutEditor", () => ({
  hasUnsavedShortcuts: () => false,
  discardShortcutDrafts: vi.fn(),
  default: () => null,
}));

vi.mock("../../stores/global/hotkey", async () => {
  const { createRoot, createSignal } = await import("solid-js");
  return createRoot(() => {
    const [taken, setTaken] = createSignal(false);
    const [unsupported, setUnsupported] = createSignal(false);
    chord.setTaken = setTaken;
    chord.setUnsupported = setUnsupported;
    return {
      SHIPPED_WINDOW_CHORD: "CmdOrCtrl+Shift+Space",
      SHIPPED_CAPTURE_CHORD: "CmdOrCtrl+Shift+Alt+Space",
      hotkeyStore: {
        chord: () => "CmdOrCtrl+Shift+Space",
        isTaken: taken,
        isUnsupported: unsupported,
      },
      captureHotkeyStore: {
        chord: () => "CmdOrCtrl+Shift+Alt+Space",
        isTaken: () => false,
        isUnsupported: () => false,
      },
    };
  });
});

vi.mock("../../stores/global/login-item", async () => {
  const { createRoot, createSignal } = await import("solid-js");
  return createRoot(() => {
    const [enabled, setEnabled] = createSignal(false);
    login.setEnabled = setEnabled;
    return {
      loginItemStore: {
        enabled,
        busy: () => false,
        load: login.load,
        set: login.set,
      },
    };
  });
});

vi.mock("../../stores/global/theme", () => ({
  themeStore: {
    setPreset: mocks.setPreset,
    setAppearance: vi.fn(),
    accentApplies: () => mocks.accentApplies(),
    polarity: () => mocks.polarity(),
    activePreset: () => ({ id: mocks.activePresetId() }),
  },
}));


import SettingsModal, {
  openSettings,
  closeSettings,
  openWindowChordSetting,
} from "../../components/SettingsModal/SettingsModal";
import { rankSettings } from "../../settings";

function baseConfig(): WritConfig {
  return {
    hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
    sidebar: { toggle: "CmdOrCtrl+\\", default_visible: false, position: "left", open: false, width: 240, collapsed: [], hidden: [] },
    panel: { open: false, width: 240 },
    chat_panel: { open: false, width: 380 },
    first_run: { hint_dismissed: false },
    editor: { font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_editing: true, status_bar: false, status_bar_counts: false },
    window: { width: 1100, height: 720, maximized: false },
    keybindings: {},
    history: { max_entries: 500 },
    theme: { preset: "warp-dark", overrides: {} },
    appearance: { polarity: "system", accent: "pine", prose_face: "system", interface_text_size: null },
    files: { default_extension: "txt" },
    workspace: { root: null },
    inbox: { path: null, focus: true },
    updater: { auto_check: true },
    ai: { provider: "ollama", base_url: "", model: "", consented_hosts: [], rewrite: { enabled: false }, chat: { enabled: false, model: "", model_provider: "" } },
    mcp: { enabled: false, approved_clients: [] },
    apps: { connections: false, graph: false, tags: false },
    spelling: { enabled: false, dialect: "american", ignored_words: [] },
    preview: {
      default_layout_html: "split",
      default_layout_markdown: "inline",
      live_render_threshold_mb: 1,
      render_confirm_threshold_mb: 5,
      render_refuse_threshold_mb: 50,
      debounce_ms: 200,
    },
  };
}

async function openGeneralSection(container: Element) {
  await configStore.save(baseConfig());
  openSettings("general");
  await waitFor(() => expect(container.querySelector("[data-section='general']")).not.toBeNull());
}

function windowRow(container: Element): HTMLElement {
  const row = container.querySelector<HTMLElement>("[data-setting-id='hotkey.toggle']");
  expect(row, "General carries the window's chord").not.toBeNull();
  return row!;
}

describe("SettingsModal General: the window's chord and open at login", () => {
  beforeEach(() => {
    mocks.fetchDefaultAppTypes.mockReset().mockResolvedValue([]);
    login.load.mockClear();
    login.set.mockClear();
    login.setEnabled(false);
    chord.setTaken(false);
    chord.setUnsupported(false);
  });

  afterEach(() => {
    closeSettings();
    cleanup();
  });

  it("shows the window's chord with no caution while Writ holds it", async () => {
    const { container } = render(() => <SettingsModal />);
    await openGeneralSection(container);

    expect(windowRow(container).querySelector(".settings-row-caution")).toBeNull();
    expect(windowRow(container).querySelector(".kbd-chord-muted")).toBeNull();
  });

  it("says another app holds the chord, mutes it and keeps its recorder", async () => {
    chord.setTaken(true);
    const { container } = render(() => <SettingsModal />);
    await openGeneralSection(container);

    const caution = windowRow(container).querySelector(".settings-row-caution");
    expect(caution?.textContent).toBe("Another app is using this shortcut.");
    expect(windowRow(container).querySelector(".kbd-chord-muted")).not.toBeNull();
    expect(windowRow(container).querySelector("[data-action='record-global-chord']")).not.toBeNull();
  });

  it("opens Settings listening on the window's chord when asked to", async () => {
    chord.setTaken(true);
    await configStore.save(baseConfig());
    const { container } = render(() => <SettingsModal />);
    openWindowChordSetting(true);

    await waitFor(() =>
      expect(windowRow(container).querySelector(".settings-chord-note")?.textContent).toBe(
        "Press a key…",
      ),
    );
    expect(windowRow(container).classList.contains("settings-row-highlight")).toBe(true);
  });

  it("opens Settings at the window's chord without listening when not asked to", async () => {
    await configStore.save(baseConfig());
    const { container } = render(() => <SettingsModal />);
    openWindowChordSetting(false);

    await waitFor(() => expect(windowRow(container)).not.toBeNull());
    expect(
      windowRow(container).querySelector("[data-action='record-global-chord']")!.textContent,
    ).toBe("Record");
  });

  it("says a Wayland session cannot pass the chord on", async () => {
    chord.setUnsupported(true);
    const { container } = render(() => <SettingsModal />);
    await openGeneralSection(container);

    expect(windowRow(container).querySelector(".settings-row-caution")?.textContent).toBe(
      "Wayland doesn't pass this shortcut to Writ from other apps.",
    );
  });

  it("reads open at login from the OS and asks it on a click", async () => {
    login.setEnabled(true);
    const { container } = render(() => <SettingsModal />);
    await openGeneralSection(container);

    expect(login.load).toHaveBeenCalled();
    const toggle = container.querySelector<HTMLButtonElement>("[data-setting='launch_at_login']")!;
    expect(toggle.getAttribute("aria-checked")).toBe("true");

    fireEvent.click(toggle);
    expect(login.set).toHaveBeenCalledWith(false);
  });

  it("finds open at login by searching for startup", () => {
    expect(rankSettings("startup").map((entry) => entry.id)).toContain("general.launch_at_login");
    expect(rankSettings("hotkey").map((entry) => entry.id)).toContain("hotkey.toggle");
  });
});
