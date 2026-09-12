import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";
import { SIDEBAR_SECTIONS } from "../../lib/sidebar-sections";

const TEST_CLAIMABLE_TYPE = {
  id: "markdown",
  label: "Markdown",
  exts: ["md", "markdown"],
  utis: ["net.daringfireball.markdown"],
};

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
  openShortcutEditor: vi.fn(),
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
  notesFolder: vi.fn(),
  notesLoadFolder: vi.fn(),
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

/** Endpoint state for a hosted provider, defaulting to the operator's reported
 * situation: reachable and allowed, but no consent recorded. */
function hostedEndpoint(overrides: Record<string, unknown> = {}) {
  return {
    host: "api.deepseek.com",
    host_port: "api.deepseek.com",
    is_hosted: true,
    is_allowed: true,
    is_consented: false,
    key_state: { is_set: true, memory_only: false },
    ...overrides,
  };
}

vi.mock("../../services/tauri", () => ({
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

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: mocks.writeClipboardText,
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
  openShortcutEditor: mocks.openShortcutEditor,
  default: () => null,
}));

vi.mock("../../stores/global/theme", () => ({
  themeStore: {
    setPreset: mocks.setPreset,
    setAppearance: vi.fn(),
    accentApplies: () => mocks.accentApplies(),
    polarity: () => mocks.polarity(),
    activePreset: () => ({ id: mocks.activePresetId() }),
  },
}));


import SettingsModal, { openSettings, closeSettings } from "../../components/SettingsModal/SettingsModal";
import { clearDefaultAppSupport } from "../../stores/global/default-app-support";

function baseConfig(): WritConfig {
  return {
    hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
    sidebar: { toggle: "CmdOrCtrl+\\", default_visible: false, position: "left", open: false, width: 240, collapsed: [], hidden: [] },
    panel: { open: false, width: 240 },
    chat_panel: { open: false, width: 380 },
    first_run: { hint_dismissed: false },
    editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_typography: true, markdown_editing: true, status_bar: false },
    window: { width: 1100, height: 720, maximized: false },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "warp-dark", overrides: {} },
    appearance: { polarity: "system", accent: "pine", prose_face: "system", interface_text_size: null },
    commands: { usage: {} },
    workspace: { root: null },
    inbox: { path: null, focus: true },
    updater: { auto_check: true },
    ai: { enabled: false, preset: "ollama", base_url: "http://localhost:11434/v1", model: "", consented_hosts: [], chat: { enabled: false, provider: "openai_compatible", base_url: "http://localhost:11434/v1", model: "" } },
    mcp: { enabled: false, approved_clients: [] },
    spelling: { enabled: false, dialect: "american", ignored_words: [] },
    preview: {
      default_layout_html: "split",
      default_layout_markdown: "split",
      live_render_threshold_mb: 1,
      render_confirm_threshold_mb: 5,
      render_refuse_threshold_mb: 50,
      debounce_ms: 200,
      run_scripts: true,
    },
  };
}

const SIDEBAR_SETTINGS = ["sidebar_folder", "sidebar_tags", "sidebar_inbox", "sidebar_recent"];

/** The switches read and write the real store; `save` only seeds it. */
async function seedConfig(config: WritConfig) {
  await configStore.save(config);
}

function switches(container: Element): HTMLButtonElement[] {
  return Array.from(
    container.querySelectorAll<HTMLButtonElement>("[data-section='sidebar'] [role='switch']"),
  );
}

async function openSidebarSection(container: Element) {
  openSettings("sidebar");
  await waitFor(() => expect(container.querySelector("[data-section='sidebar']")).not.toBeNull());
}

async function openAndSearch(container: Element, term: string) {
  openSettings();
  await waitFor(() => expect(container.querySelector(".settings-search-input")).not.toBeNull());
  const input = container.querySelector<HTMLInputElement>(".settings-search-input")!;
  fireEvent.input(input, { target: { value: term } });
}

describe("SettingsModal sidebar section", () => {
  const setHidden = vi.spyOn(configStore, "setSidebarSectionHidden");

  beforeEach(async () => {
    setHidden.mockClear();
    mocks.accentApplies.mockReset().mockReturnValue(true);
    mocks.activePresetId.mockReset().mockReturnValue("warp-dark");
    mocks.polarity.mockReset().mockReturnValue("light");
    mocks.setPreset.mockReset();
    mocks.openThemeEditor.mockReset();
    mocks.openShortcutEditor.mockReset();
    mocks.fetchDefaultAppStatus.mockReset().mockResolvedValue({ status: "unsupported" });
    mocks.claimDefaultApp.mockReset().mockResolvedValue(undefined);
    mocks.fetchDefaultAppTypes.mockReset().mockResolvedValue([TEST_CLAIMABLE_TYPE]);
    mocks.fetchCliStatus.mockReset().mockResolvedValue({ installed: false });
    mocks.fetchStorageInfo.mockReset().mockResolvedValue({ db_path: "/home/user/.writ/writ.db", dir: "/home/user/.writ" });
    mocks.revealStoragePath.mockReset().mockResolvedValue(undefined);
    mocks.copyStoragePath.mockReset().mockResolvedValue(undefined);
    mocks.writeClipboardText.mockReset().mockResolvedValue(undefined);
    mocks.openThirdPartyNoticesBuffer
      .mockReset()
      .mockResolvedValue({ doc: { id: "notices-buffer" }, reused: false });
    mocks.setActiveTabId.mockReset();
    mocks.requestExternalReload.mockReset();
    mocks.notesFolder.mockReset().mockReturnValue({
      path: "/home/user/Writ",
      display_path: "~/Writ",
      fallback: null,
      sync_provider: null,
    });
    mocks.notesLoadFolder.mockReset().mockResolvedValue(undefined);
    mocks.notesShowInFileManager.mockReset().mockResolvedValue(undefined);
    mocks.notesCopyPath.mockReset().mockResolvedValue(undefined);
    mocks.notesMove.mockReset().mockResolvedValue(null);
    mocks.aiEndpointState.mockReset().mockResolvedValue(hostedEndpoint());
    mocks.aiConsentHost.mockReset().mockResolvedValue(hostedEndpoint({ is_consented: true }));
    clearDefaultAppSupport();
    await seedConfig(baseConfig());
  });

  afterEach(() => {
    closeSettings();
    cleanup();
  });

  it("lists Sidebar in the nav between Appearance and Updates", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const labels = Array.from(container.querySelectorAll(".settings-nav-item")).map((n) => n.textContent);
    const at = labels.indexOf("Sidebar");
    expect(at).toBeGreaterThan(0);
    expect(labels[at - 1]).toBe("Appearance");
    expect(labels[at + 1]).toBe("Updates");
  });

  it("shows one switch per section, all on by default", async () => {
    const { container } = render(() => <SettingsModal />);
    await openSidebarSection(container);
    const controls = switches(container);
    expect(controls.map((c) => c.dataset.setting)).toEqual(SIDEBAR_SETTINGS);
    expect(controls.map((c) => c.getAttribute("aria-checked"))).toEqual(["true", "true", "true", "true"]);
    expect(controls.map((c) => c.getAttribute("aria-label"))).toEqual([
      "Show notes",
      "Show tags",
      "Show watched folder",
      "Show recently closed",
    ]);
  });

  it("follows the sidebar's order", () => {
    expect(SIDEBAR_SETTINGS).toEqual(SIDEBAR_SECTIONS.map((id) => `sidebar_${id}`));
  });

  it("reads a hidden section as off", async () => {
    const base = baseConfig();
    await seedConfig({ ...base, sidebar: { ...base.sidebar, hidden: ["tags"] } });
    const { container } = render(() => <SettingsModal />);
    await openSidebarSection(container);
    const byId = new Map(switches(container).map((c) => [c.dataset.setting, c.getAttribute("aria-checked")]));
    expect(byId.get("sidebar_tags")).toBe("false");
    expect(byId.get("sidebar_folder")).toBe("true");
    expect(byId.get("sidebar_inbox")).toBe("true");
    expect(byId.get("sidebar_recent")).toBe("true");
  });

  it("hides a section on click and flips the switch", async () => {
    const { container } = render(() => <SettingsModal />);
    await openSidebarSection(container);
    const tags = container.querySelector<HTMLButtonElement>("[data-setting='sidebar_tags']")!;
    fireEvent.click(tags);
    expect(setHidden).toHaveBeenCalledWith("tags", true);
    await waitFor(() => expect(tags.getAttribute("aria-checked")).toBe("false"));
    expect(configStore.isSidebarSectionHidden("tags")).toBe(true);

    fireEvent.click(tags);
    expect(setHidden).toHaveBeenLastCalledWith("tags", false);
    await waitFor(() => expect(tags.getAttribute("aria-checked")).toBe("true"));
  });

  it("surfaces the recently closed row in search", async () => {
    const { container } = render(() => <SettingsModal />);
    await openAndSearch(container, "recently closed");
    await waitFor(() => {
      expect(container.querySelector("[data-setting-id='sidebar.recent']")).not.toBeNull();
    });
    expect(container.querySelector("[data-setting-id='sidebar.tags']")).toBeNull();
  });

  it("does not answer a search for the config id", async () => {
    const { container } = render(() => <SettingsModal />);
    await openAndSearch(container, "inbox");
    await waitFor(() => expect(container.querySelector(".settings-empty")).not.toBeNull());
    expect(container.querySelector("[data-setting-id^='sidebar.']")).toBeNull();
  });
});
