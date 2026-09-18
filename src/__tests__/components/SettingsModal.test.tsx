import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";

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
  save: vi.fn().mockResolvedValue(undefined),
  config: vi.fn(),
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
  aiProviders: vi.fn(),
  aiListModels: vi.fn(),
  aiSetProvider: vi.fn(),
  aiProbeLocal: vi.fn(),
  aiOpenrouterConnect: vi.fn(),
  aiOpenrouterCancel: vi.fn(),
  openExternalUrl: vi.fn().mockResolvedValue(undefined),
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
    provider: "deepseek",
    key_state: { is_set: true, memory_only: false },
    ...overrides,
  };
}

/** A catalog the provider itself answered. */
function listed(provider: string, models: string[]) {
  return { provider, models, source: "live" as const, error: null };
}

/** A catalog that fell back to the table's suggestions. */
function suggested(provider: string) {
  const row = TEST_PROVIDERS.find((r) => r.id === provider);
  return {
    provider,
    models: (row?.curated_models ?? []) as string[],
    source: "curated" as const,
    error: { kind: "unreachable" as const },
  };
}

/** Rendered text, with the line wrapping the markup adds taken back out. */
function collapse(text: string | null): string {
  return (text ?? "").replace(/\s+/g, " ").trim();
}

/** The provider table, as the AI section reads it (ADR-040 section 2). */
function providerRow(overrides: Record<string, unknown> = {}) {
  return {
    id: "ollama",
    label: "Ollama",
    group: "local",
    wire: "openai",
    base_url: "http://localhost:11434/v1",
    models_url: "http://localhost:11434/api/tags",
    key_page_url: null,
    default_model: "",
    needs_key: false,
    supports_connect: false,
    probe_port: 11434,
    curated_models: [] as string[],
    ...overrides,
  };
}

const TEST_PROVIDERS = [
  providerRow(),
  providerRow({ id: "lmstudio", label: "LM Studio", probe_port: 1234 }),
  providerRow({
    id: "deepseek",
    label: "DeepSeek",
    group: "hosted",
    base_url: "https://api.deepseek.com",
    key_page_url: "https://platform.deepseek.com/api_keys",
    default_model: "deepseek-chat",
    curated_models: ["deepseek-chat", "deepseek-reasoner"],
    needs_key: true,
    probe_port: null,
  }),
  providerRow({
    id: "openrouter",
    label: "OpenRouter",
    group: "hosted",
    base_url: "https://openrouter.ai/api/v1",
    key_page_url: "https://openrouter.ai/settings/keys",
    default_model: "meta-llama/llama-3.3-70b-instruct",
    needs_key: true,
    supports_connect: true,
    probe_port: null,
  }),
  providerRow({
    id: "custom",
    label: "Custom (OpenAI-compatible)",
    group: "custom",
    base_url: "",
    models_url: "",
    needs_key: false,
    probe_port: null,
  }),
];

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
  aiProviders: mocks.aiProviders,
  aiListModels: mocks.aiListModels,
  aiSetProvider: mocks.aiSetProvider,
  aiProbeLocal: mocks.aiProbeLocal,
  aiOpenrouterConnect: mocks.aiOpenrouterConnect,
  aiOpenrouterCancel: mocks.aiOpenrouterCancel,
  openExternalUrl: mocks.openExternalUrl,
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

vi.spyOn(configStore, "save").mockImplementation(mocks.save);
vi.spyOn(configStore, "config").mockImplementation(mocks.config);

import SettingsModal, { openSettings, closeSettings } from "../../components/SettingsModal/SettingsModal";
import { aiConnectionStore } from "../../stores/global/ai-connection";
import {
  aiProvidersStore,
  groupProviders,
  type AiProviderInfo,
  type ProviderGroupedOptions,
} from "../../stores/global/ai-providers";
import { SETTINGS_INDEX, SECTION_ORDER } from "../../settings";
import { clearDefaultAppSupport, probeDefaultAppSupport } from "../../stores/global/default-app-support";

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
  ai: { provider: "ollama", base_url: "", model: "", consented_hosts: [], rewrite: { enabled: false }, chat: { enabled: false, model: "", model_provider: "" } },
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

describe("SettingsModal", () => {
  beforeEach(() => {
    mocks.save.mockReset().mockResolvedValue(undefined);
    mocks.accentApplies.mockReset().mockReturnValue(true);
    mocks.activePresetId.mockReset().mockReturnValue("warp-dark");
    mocks.polarity.mockReset().mockReturnValue("light");
    mocks.setPreset.mockReset();
    mocks.config.mockReset().mockReturnValue(baseConfig());
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
    mocks.aiProviders.mockReset().mockResolvedValue(TEST_PROVIDERS);
    mocks.aiListModels.mockReset().mockImplementation((provider: string) => suggested(provider));
    mocks.aiSetProvider.mockReset().mockResolvedValue(undefined);
    mocks.aiProbeLocal.mockReset().mockResolvedValue({ ollama: false, lmstudio: false });
    mocks.aiOpenrouterConnect.mockReset().mockResolvedValue({ is_set: true, memory_only: false });
    mocks.aiOpenrouterCancel.mockReset().mockResolvedValue(undefined);
    mocks.openExternalUrl.mockReset().mockResolvedValue(undefined);
    mocks.notesFolder
      .mockReset()
      .mockReturnValue({
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
  });

  afterEach(() => {
    closeSettings();
    cleanup();
  });

  it("renders nothing when closed", () => {
    const { container } = render(() => <SettingsModal />);
    expect(container.querySelector("[role='dialog']")).toBeNull();
  });

  it("renders dialog when opened", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => {
      expect(container.querySelector("[role='dialog']")).not.toBeNull();
    });
  });

  it("has aria-modal and aria-labelledby", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => {
      const dialog = container.querySelector("[role='dialog']");
      expect(dialog).not.toBeNull();
      expect(dialog!.getAttribute("aria-modal")).toBe("true");
      expect(dialog!.getAttribute("aria-labelledby")).toBeTruthy();
    });
  });

  it("closes on Escape key", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector("[role='dialog']")).not.toBeNull());
    fireEvent.keyDown(container.querySelector("[role='dialog']")!, { key: "Escape" });
    await waitFor(() => expect(container.querySelector("[role='dialog']")).toBeNull());
  });

  it("closes when clicking overlay", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-overlay")).not.toBeNull());
    fireEvent.click(container.querySelector(".settings-overlay")!);
    await waitFor(() => expect(container.querySelector("[role='dialog']")).toBeNull());
  });

  it("shows a nav item for every section", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll(".settings-nav-item");
    expect(navItems.length).toBe(SECTION_ORDER.length);
  });

  it("shows the section the nav rail leads with by default", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => {
      expect(container.querySelector(`[data-section='${SECTION_ORDER[0]}']`)).not.toBeNull();
    });
    expect(container.querySelector("[data-section='editor']")).toBeNull();
  });

  it("switches to Files section on nav click", async () => {
    // Files renders only once a type reports claimable, so the nav has
    // something to switch to.
    mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "no_handler" });
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
    const filesItem = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("files"));
    expect(filesItem).toBeTruthy();
    fireEvent.click(filesItem!);
    await waitFor(() => {
      expect(container.querySelector("[data-section='files']")).not.toBeNull();
    });
  });

  it("saves font size change", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings("editor");
    await waitFor(() => expect(container.querySelector("[data-section='editor']")).not.toBeNull());
    const fontSizeInput = container.querySelector<HTMLInputElement>("[data-setting='font_size']");
    expect(fontSizeInput).not.toBeNull();
    fireEvent.change(fontSizeInput!, { target: { value: "16" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.font_size).toBe(16);
  });

  it("clamps font size to valid range", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings("editor");
    await waitFor(() => expect(container.querySelector("[data-section='editor']")).not.toBeNull());
    const fontSizeInput = container.querySelector<HTMLInputElement>("[data-setting='font_size']");
    fireEvent.change(fontSizeInput!, { target: { value: "200" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.font_size).toBeLessThanOrEqual(72);
  });

  it("saves word wrap toggle", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings("editor");
    await waitFor(() => expect(container.querySelector("[data-section='editor']")).not.toBeNull());
    const toggle = container.querySelector<HTMLButtonElement>("[data-setting='word_wrap']");
    expect(toggle).not.toBeNull();
    fireEvent.click(toggle!);
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.word_wrap).toBe(false);
  });

  it("saves tab size change", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings("editor");
    await waitFor(() => expect(container.querySelector("[data-section='editor']")).not.toBeNull());
    const tabSizeInput = container.querySelector<HTMLInputElement>("[data-setting='tab_size']");
    expect(tabSizeInput).not.toBeNull();
    fireEvent.change(tabSizeInput!, { target: { value: "4" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.tab_size).toBe(4);
  });

  it("opens ThemeEditor from Appearance section", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
    const appearanceNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("appearance"));
    fireEvent.click(appearanceNav!);
    await waitFor(() => expect(container.querySelector("[data-section='appearance']")).not.toBeNull());
    const editBtn = container.querySelector<HTMLButtonElement>("[data-action='open-theme-editor']");
    expect(editBtn).not.toBeNull();
    fireEvent.click(editBtn!);
    expect(mocks.openThemeEditor).toHaveBeenCalledTimes(1);
  });

  it("opens ShortcutEditor from Shortcuts section", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
    const shortcutsNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("shortcuts"));
    fireEvent.click(shortcutsNav!);
    await waitFor(() => expect(container.querySelector("[data-section='shortcuts']")).not.toBeNull());
    const editBtn = container.querySelector<HTMLButtonElement>("[data-action='open-shortcut-editor']");
    expect(editBtn).not.toBeNull();
    fireEvent.click(editBtn!);
    expect(mocks.openShortcutEditor).toHaveBeenCalledTimes(1);
  });

  it("saves theme preset from Appearance section", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
    const appearanceNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("appearance"));
    fireEvent.click(appearanceNav!);
    await waitFor(() => expect(container.querySelector("[data-section='appearance']")).not.toBeNull());
    const presetSelect = container.querySelector<HTMLSelectElement>("[data-setting='theme_preset']");
    expect(presetSelect).not.toBeNull();
    fireEvent.change(presetSelect!, { target: { value: "warp-light" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.theme.preset).toBe("warp-light");
  });

  // Choosing a preset is a light/dark choice. Left on system, the pair swap
  // rendered the dark half under the name the user had just picked, so the
  // pinned polarity has to reach disk with the preset, in one write.
  it("saves the preset and the polarity it pins in one write", async () => {
    mocks.polarity.mockReturnValue("light");
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    const presetSelect = container.querySelector<HTMLSelectElement>("[data-setting='theme_preset']");
    fireEvent.change(presetSelect!, { target: { value: "warp-light" } });
    expect(mocks.setPreset).toHaveBeenCalledWith("warp-light");
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.theme.preset).toBe("warp-light");
    expect(saved.appearance.polarity).toBe("light");
  });

  // The stored preset is one half of a pair; polarity picks the half that
  // renders. The row named the stored half, so under a dark system the Theme
  // row read "Writ Light" over a dark app.
  it("the theme row names the half of the pair that renders", async () => {
    mocks.config.mockReturnValue({
      ...baseConfig(),
      theme: { preset: "writ-light", overrides: {} },
      appearance: { polarity: "dark", accent: "pine", prose_face: "system", interface_text_size: null },
    });
    mocks.activePresetId.mockReturnValue("writ-dark");
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    const presetSelect = container.querySelector<HTMLSelectElement>("[data-setting='theme_preset']");
    expect(presetSelect!.value).toBe("writ-dark");
    expect(presetSelect!.selectedOptions[0].textContent).toBe("Writ Dark");
  });

  it("the theme row names the light half under a light polarity", async () => {
    mocks.config.mockReturnValue({
      ...baseConfig(),
      theme: { preset: "writ-dark", overrides: {} },
      appearance: { polarity: "light", accent: "pine", prose_face: "system", interface_text_size: null },
    });
    mocks.activePresetId.mockReturnValue("writ-light");
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    const presetSelect = container.querySelector<HTMLSelectElement>("[data-setting='theme_preset']");
    expect(presetSelect!.value).toBe("writ-light");
    expect(presetSelect!.selectedOptions[0].textContent).toBe("Writ Light");
  });

  async function openAppearance(container: HTMLElement) {
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
    const appearanceNav = Array.from(navItems).find((n) =>
      n.textContent?.toLowerCase().includes("appearance"),
    );
    fireEvent.click(appearanceNav!);
    await waitFor(() => expect(container.querySelector("[data-section='appearance']")).not.toBeNull());
  }

  function accentSwatches(container: HTMLElement) {
    return Array.from(
      container.querySelectorAll<HTMLButtonElement>("[data-setting='appearance_accent'] .settings-accent"),
    );
  }

  it("offers the accent while the theme defers its highlight to the setting", async () => {
    mocks.accentApplies.mockReturnValue(true);
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    const swatches = accentSwatches(container);
    expect(swatches).toHaveLength(6);
    const byAccent = (id: string) => swatches.find((s) => s.dataset.accent === id);
    expect(byAccent("pine")!.disabled).toBe(false);
    expect(byAccent("writ-blue")!.disabled).toBe(false);
    expect(byAccent("terracotta")!.disabled).toBe(false);
    expect(byAccent("slate")!.disabled).toBe(false);
    expect(byAccent("plum")!.disabled).toBe(false);
    expect(byAccent("gold")!.disabled).toBe(false);
    const row = container.querySelector("[data-setting-id='appearance.accent']");
    expect(row!.querySelector(".settings-row-caution")).toBeNull();

    fireEvent.click(swatches.find((s) => s.dataset.accent === "plum")!);
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.appearance.accent).toBe("plum");
  });

  it("marks the active accent with aria-pressed and leaves the rest unpressed", async () => {
    mocks.accentApplies.mockReturnValue(true);
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    const swatches = accentSwatches(container);
    const pressed = swatches.filter((s) => s.getAttribute("aria-pressed") === "true");
    expect(pressed).toHaveLength(1);
    expect(pressed[0].dataset.accent).toBe("pine");
    expect(pressed[0].getAttribute("aria-label")).toBe("Pine");
  });

  it("disables the accent and says why while the theme sets its own", async () => {
    mocks.accentApplies.mockReturnValue(false);
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    expect(accentSwatches(container).every((s) => s.disabled)).toBe(true);
    const row = container.querySelector("[data-setting-id='appearance.accent']");
    expect(row!.querySelector(".settings-row-caution")!.textContent).toBe(
      "The current theme sets its own accent.",
    );
  });

  it("writes appearance.polarity from the segmented control", async () => {
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    const dark = container.querySelector<HTMLButtonElement>(
      "[data-setting='appearance_polarity'] [data-option='dark']",
    );
    expect(dark).not.toBeNull();
    fireEvent.click(dark!);
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.appearance.polarity).toBe("dark");
  });

  it("writes appearance.prose_face from the prose face select", async () => {
    const { container } = render(() => <SettingsModal />);
    await openAppearance(container);
    const face = container.querySelector<HTMLSelectElement>("[data-setting='appearance_prose_face']");
    expect(face).not.toBeNull();
    fireEvent.change(face!, { target: { value: "quattro" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.appearance.prose_face).toBe("quattro");
  });

  it("writes editor.status_bar from the status bar switch", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings("editor");
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const statusBar = container.querySelector<HTMLButtonElement>("[data-setting='status_bar']");
    expect(statusBar).not.toBeNull();
    expect(statusBar!.getAttribute("role")).toBe("switch");
    expect(statusBar!.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(statusBar!);
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.status_bar).toBe(true);
  });

  it("saves preview run_scripts toggle from Preview section", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
    const previewNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("preview"));
    fireEvent.click(previewNav!);
    await waitFor(() => expect(container.querySelector("[data-section='preview']")).not.toBeNull());
    const scriptsToggle = container.querySelector<HTMLButtonElement>("[data-setting='run_scripts']");
    expect(scriptsToggle).not.toBeNull();
    fireEvent.click(scriptsToggle!);
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.preview.run_scripts).toBe(false);
  });

  describe("Files section — the file-types row", () => {
    async function openFilesNav(container: Element) {
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
      const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
      const filesNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("files"));
      fireEvent.click(filesNav!);
    }

    async function openFilesSection(container: Element) {
      await openFilesNav(container);
      await waitFor(() => expect(container.querySelector("[data-section='files']")).not.toBeNull());
    }

    function filesHeading(container: Element): Element | undefined {
      return Array.from(container.querySelectorAll(".settings-section-label")).find(
        (el) => el.textContent === "Files",
      );
    }

    // The macOS path: the startup probe answers before Settings can open, so
    // heading and row are there to read the moment the nav switches.
    it("shows the Files heading on first render once support is known", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "no_handler" });
      await probeDefaultAppSupport();
      mocks.fetchDefaultAppTypes.mockReturnValue(new Promise(() => {}));

      const { container } = render(() => <SettingsModal />);
      await openFilesNav(container);
      expect(filesHeading(container)).toBeDefined();
      expect(container.querySelector("[data-section='files']")).not.toBeNull();
      expect(container.querySelector("[data-setting-id='files.default_app']")).not.toBeNull();
    });

    // Support can be withdrawn: a type the startup probe counted answers
    // unsupported here, which empties the registry and takes the row with it.
    it("drops the Files heading when a known type turns out unclaimable", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "no_handler" });
      await probeDefaultAppSupport();
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "unsupported" });

      const { container } = render(() => <SettingsModal />);
      await openFilesNav(container);
      expect(container.querySelector("[data-section='files']")).not.toBeNull();
      await waitFor(() => expect(container.querySelector("[data-section='files']")).toBeNull());
      expect(filesHeading(container)).toBeUndefined();
      expect(container.querySelector("[data-default-app-type]")).toBeNull();
    });

    // A heading with nothing under it says less than no heading at all.
    it("shows no Files heading when every type answers unsupported", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "unsupported" });
      const { container } = render(() => <SettingsModal />);
      await openFilesNav(container);
      await waitFor(() => expect(mocks.fetchDefaultAppStatus).toHaveBeenCalled());
      expect(filesHeading(container)).toBeUndefined();
      expect(container.querySelector("[data-section='files']")).toBeNull();
      expect(container.querySelector("[data-setting-id='files.default_app']")).toBeNull();
    });

    // Opening Settings the instant the app starts: no heading appears and then
    // leaves, because the list only ever grows as types report in.
    it("shows no Files heading while the type probe is still pending", async () => {
      mocks.fetchDefaultAppTypes.mockReturnValue(new Promise(() => {}));
      const { container } = render(() => <SettingsModal />);
      await openFilesNav(container);
      expect(filesHeading(container)).toBeUndefined();
      expect(container.querySelector("[data-section='files']")).toBeNull();
      expect(container.querySelector("[data-setting-id='files.default_app']")).toBeNull();
    });

    it("shows no Files heading when the type probe fails", async () => {
      mocks.fetchDefaultAppTypes.mockRejectedValue(new Error("no IPC"));
      const { container } = render(() => <SettingsModal />);
      await openFilesNav(container);
      await waitFor(() => expect(mocks.fetchDefaultAppTypes).toHaveBeenCalled());
      expect(filesHeading(container)).toBeUndefined();
      expect(container.querySelector("[data-section='files']")).toBeNull();
      expect(container.querySelector("[data-setting-id='files.default_app']")).toBeNull();
    });

    it("offers one row holding a box per claimable type", async () => {
      mocks.fetchDefaultAppTypes.mockResolvedValue([
        TEST_CLAIMABLE_TYPE,
        { id: "html", label: "Web pages", exts: ["html"], utis: ["public.html"] },
      ]);
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "no_handler" });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => {
        const ids = Array.from(
          container.querySelectorAll<HTMLInputElement>("[data-default-app-type]"),
        ).map((el) => el.dataset.defaultAppType);
        expect(ids).toEqual(["markdown", "html"]);
      });
      expect(container.querySelectorAll("[data-setting-id^='files.default_app']").length).toBe(1);
    });

    it("checks and locks a type Writ already holds", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "is_default" });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => {
        const box = container.querySelector<HTMLInputElement>(
          "[data-default-app-type='markdown']",
        );
        expect(box).not.toBeNull();
        expect(box!.checked).toBe(true);
        expect(box!.disabled).toBe(true);
      });
    });

    it("names the app that holds a type Writ does not", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: "TextEdit" });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => {
        const box = container.querySelector<HTMLInputElement>(
          "[data-default-app-type='markdown']",
        );
        expect(box).not.toBeNull();
        expect(box!.checked).toBe(false);
        expect(box!.disabled).toBe(false);
      });
      expect(container.querySelector(".settings-file-type-owner")!.textContent).toContain(
        "TextEdit",
      );
    });

    it("leaves the owner unnamed when the OS does not name it", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: null });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() =>
        expect(container.querySelector("[data-default-app-type='markdown']")).not.toBeNull(),
      );
      expect(container.querySelector(".settings-file-type-owner")).toBeNull();
    });

    // The claim is the only direction Launch Services offers, so the box has to
    // round-trip: unchecked for a type another app holds, checked once Writ does.
    it("claims each type from its own box and reflects the answer", async () => {
      for (const typeId of ["markdown", "html"]) {
        vi.useFakeTimers();
        mocks.fetchDefaultAppTypes.mockResolvedValue([
          TEST_CLAIMABLE_TYPE,
          { id: "html", label: "Web pages", exts: ["html"], utis: ["public.html"] },
        ]);
        mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: "TextEdit" });
        mocks.claimDefaultApp.mockResolvedValue(undefined);
        const { container, unmount } = render(() => <SettingsModal />);
        await openFilesSection(container);
        const selector = `[data-default-app-type='${typeId}']`;
        await waitFor(() =>
          expect(container.querySelector<HTMLInputElement>(selector)!.checked).toBe(false),
        );

        mocks.fetchDefaultAppStatus.mockResolvedValueOnce({ status: "is_default" });
        fireEvent.click(container.querySelector<HTMLInputElement>(selector)!);
        await waitFor(() => expect(mocks.claimDefaultApp).toHaveBeenCalledWith(typeId));

        await vi.runAllTimersAsync();
        await waitFor(() => {
          const box = container.querySelector<HTMLInputElement>(selector)!;
          expect(box.checked).toBe(true);
          expect(box.disabled).toBe(true);
        });

        vi.useRealTimers();
        unmount();
        closeSettings();
        vi.clearAllMocks();
      }
    });
  });

  describe("Advanced section", () => {
    async function openAdvancedSection(container: Element) {
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
      const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
      const advancedNav = Array.from(navItems).find((n) =>
        n.textContent?.toLowerCase().includes("advanced"),
      );
      expect(advancedNav).toBeTruthy();
      fireEvent.click(advancedNav!);
      await waitFor(() => expect(container.querySelector("[data-section='advanced']")).not.toBeNull());
    }

    it("shows the folder Writ keeps its own files in", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAdvancedSection(container);
      await waitFor(() => {
        const path = container.querySelector("[data-storage-path]");
        expect(path).not.toBeNull();
        expect(path!.textContent).toBe("/home/user/.writ");
      });
    });

    it("copies the path on Copy path click", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAdvancedSection(container);
      await waitFor(() =>
        expect(container.querySelector("[data-storage-path]")!.textContent).toContain(".writ"),
      );
      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='storage-copy']")!);
      await waitFor(() => expect(mocks.copyStoragePath).toHaveBeenCalledWith("/home/user/.writ"));
    });

    it("opens the file manager on the show click", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAdvancedSection(container);
      await waitFor(() =>
        expect(container.querySelector("[data-action='storage-reveal']")).not.toBeNull(),
      );
      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='storage-reveal']")!);
      await waitFor(() => expect(mocks.revealStoragePath).toHaveBeenCalledTimes(1));
    });

    it("holds the rows a writer never needs", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAdvancedSection(container);
      for (const id of [
        "files.inbox_folder",
        "files.inbox_focus",
        "preview.live_threshold",
        "preview.refuse_threshold",
        "storage.location",
      ]) {
        expect(container.querySelector(`[data-setting-id='${id}']`)).not.toBeNull();
      }
    });

    it("surfaces the data folder in search by keyword", async () => {
      const { container } = render(() => <SettingsModal />);
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-search-input")).not.toBeNull());
      const input = container.querySelector<HTMLInputElement>(".settings-search-input")!;
      fireEvent.input(input, { target: { value: "database" } });
      await waitFor(() => {
        expect(container.querySelector("[data-setting-id='storage.location']")).not.toBeNull();
      });
    });

    it("keeps the licences off the settings panel", async () => {
      const { container } = render(() => <SettingsModal />);
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-search-input")).not.toBeNull());
      const input = container.querySelector<HTMLInputElement>(".settings-search-input")!;
      fireEvent.input(input, { target: { value: "copyright" } });
      await waitFor(() => expect(container.querySelector(".settings-empty")).not.toBeNull());
      expect(container.querySelector("[data-action='third-party-notices']")).toBeNull();
    });
  });

  describe("search", () => {
    async function openAndSearch(container: Element, term: string) {
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-search-input")).not.toBeNull());
      const input = container.querySelector<HTMLInputElement>(".settings-search-input")!;
      fireEvent.input(input, { target: { value: term } });
      return input;
    }

    it("hides the section nav while searching", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAndSearch(container, "font");
      await waitFor(() => expect(container.querySelector(".settings-nav")).toBeNull());
    });

    it("shows only rows matching the query across sections", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAndSearch(container, "font size");
      await waitFor(() => {
        const rows = container.querySelectorAll("[data-setting-id]");
        expect(rows.length).toBe(1);
        expect(rows[0].getAttribute("data-setting-id")).toBe("editor.font_size");
      });
    });

    it("surfaces a setting from a non-default section by keyword", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAndSearch(container, "cli");
      await waitFor(() => {
        expect(container.querySelector("[data-setting-id='files.cli']")).not.toBeNull();
        expect(container.querySelector("[data-setting-id='editor.font_size']")).toBeNull();
      });
    });

    it("keeps the rewrite connection status out of unrelated results", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAndSearch(container, "font");
      await waitFor(() => {
        expect(container.querySelector("[data-setting-id='editor.font_size']")).not.toBeNull();
        expect(container.querySelector(".settings-ai-connection")).toBeNull();
      });
    });

    it("shows an empty state when nothing matches", async () => {
      const { container } = render(() => <SettingsModal />);
      await openAndSearch(container, "zzzzz");
      await waitFor(() => {
        expect(container.querySelector(".settings-empty")).not.toBeNull();
        expect(container.querySelectorAll("[data-setting-id]").length).toBe(0);
      });
    });

    it("surfaces a default-app row for a gated-only query once support is known", async () => {
      // Regression: a query matching only platform-gated default-app rows must
      // not show a permanent false empty-state. Support resolved at startup
      // (not as a render side-effect) breaks the bootstrapping deadlock.
      mocks.fetchDefaultAppTypes.mockResolvedValue([
        { id: "config-data", label: "Config & data", exts: ["json"], utis: ["public.json"] },
      ]);
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "no_handler" });
      await probeDefaultAppSupport();

      const { container } = render(() => <SettingsModal />);
      await openAndSearch(container, "json");
      await waitFor(() => {
        expect(container.querySelector(".settings-empty")).toBeNull();
        expect(container.querySelector("[data-setting-id='files.default_app']")).not.toBeNull();
      });
    });

    it("shows the empty state for a gated-only query when the platform lacks support", async () => {
      mocks.fetchDefaultAppTypes.mockResolvedValue([
        { id: "config-data", label: "Config & data", exts: ["json"], utis: ["public.json"] },
      ]);
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "unsupported" });
      await probeDefaultAppSupport();

      const { container } = render(() => <SettingsModal />);
      await openAndSearch(container, "json");
      await waitFor(() => expect(container.querySelector(".settings-empty")).not.toBeNull());
    });

    it("restores the nav when the query is cleared", async () => {
      const { container } = render(() => <SettingsModal />);
      const input = await openAndSearch(container, "font");
      await waitFor(() => expect(container.querySelector(".settings-nav")).toBeNull());
      fireEvent.input(input, { target: { value: "" } });
      await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    });
  });

  describe("deep link", () => {
    it("opens the target section and highlights the row", async () => {
      const { container } = render(() => <SettingsModal />);
      openSettings("preview", "preview.run_scripts");
      await waitFor(() => {
        const row = container.querySelector("[data-setting-id='preview.run_scripts']");
        expect(row).not.toBeNull();
        expect(row!.classList.contains("settings-row-highlight")).toBe(true);
      });
      expect(container.querySelector("[data-section='editor']")).toBeNull();
    });
  });

  describe("index parity", () => {
    it("every rendered setting row has an index entry and every entry renders", async () => {
      // Render every claimable group, all supported, so the file-types row is
      // present for the parity comparison.
      mocks.fetchDefaultAppTypes.mockResolvedValue(
        ["plain-text", "markdown", "config-data", "source-code"].map((id) => ({
          id,
          label: id,
          exts: [id],
          utis: [`public.${id}`],
        })),
      );
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "is_default" });
      // Two AI rows are conditional: the base URL belongs to a custom server
      // and the key row is hidden for a provider on this machine. `custom` is
      // the one provider that shows both, so the parity check can see them.
      const base = baseConfig();
      mocks.config.mockReturnValue({
        ...base,
        ai: { ...base.ai, provider: "custom" },
      });
      const { container } = render(() => <SettingsModal />);
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-search-input")).not.toBeNull());
      // A query that matches every section label is impossible; instead drive
      // each section through the nav and collect the rows it renders.
      const rendered = new Set<string>();
      const navItems = Array.from(
        container.querySelectorAll<HTMLButtonElement>(".settings-nav-item"),
      );
      for (let i = 0; i < navItems.length; i++) {
        const sectionId = SECTION_ORDER[i];
        const expected = SETTINGS_INDEX.filter((e) => e.section === sectionId).length;
        fireEvent.click(navItems[i]);
        // Default-app rows load asynchronously; wait until every row this
        // section indexes has rendered before collecting.
        await waitFor(() =>
          expect(container.querySelectorAll("[data-setting-id]").length).toBe(expected),
        );
        for (const row of container.querySelectorAll("[data-setting-id]")) {
          const id = row.getAttribute("data-setting-id");
          if (id) rendered.add(id);
        }
      }
      const indexed = new Set(SETTINGS_INDEX.map((e) => e.id));
      expect([...rendered].sort()).toEqual([...indexed].sort());
    });
  });
});

describe("AI section", () => {
  function aiConfig(overrides: Record<string, unknown> = {}) {
    return {
      provider: "deepseek",
      base_url: "",
      model: "deepseek-chat",
      consented_hosts: [] as string[],
      rewrite: { enabled: false },
      chat: { enabled: false, model: "", model_provider: "" },
      ...overrides,
    };
  }

  async function openAiSection(overrides: Record<string, unknown> = {}) {
    mocks.config.mockReturnValue({ ...baseConfig(), ai: aiConfig(overrides) });
    const result = render(() => <SettingsModal />);
    openSettings("ai");
    await waitFor(() =>
      expect(result.container.querySelector('[data-setting-id="ai.provider"]')).not.toBeNull(),
    );
    return result;
  }

  beforeEach(() => {
    aiConnectionStore.reset();
    mocks.save.mockReset().mockResolvedValue(undefined);
    mocks.config.mockReset().mockReturnValue(baseConfig());
    mocks.aiProviders.mockReset().mockResolvedValue(TEST_PROVIDERS);
    mocks.aiListModels.mockReset().mockImplementation((provider: string) => suggested(provider));
    mocks.aiSetProvider.mockReset().mockResolvedValue(undefined);
    mocks.aiProbeLocal.mockReset().mockResolvedValue({ ollama: false, lmstudio: false });
    mocks.aiConsentHost.mockReset().mockResolvedValue(hostedEndpoint({ is_consented: true }));
    mocks.aiEndpointState.mockReset().mockResolvedValue(hostedEndpoint());
    mocks.openExternalUrl.mockReset().mockResolvedValue(undefined);
    mocks.aiCheckConnection.mockReset().mockResolvedValue({
      reachable: true,
      model_listed: true,
      kind: "ok",
      detail: "",
      models: [],
    });
  });

  afterEach(() => {
    closeSettings();
    cleanup();
  });

  it("carries a description under every row's label", async () => {
    const { container } = await openAiSection();
    for (const id of ["ai.provider", "ai.api_key", "ai.model", "ai.connection"]) {
      const row = container.querySelector(`[data-setting-id="${id}"]`)!;
      expect(row.querySelector(".settings-row-description")?.textContent, id).toBeTruthy();
    }
    expect(
      container
        .querySelector('[data-setting-id="ai.provider"] .settings-row-description')!
        .textContent,
    ).toBe("The service that runs the model.");
  });

  // The table is data `writ-core` owns, so the picker never spells a provider
  // out itself: a row added in Rust appears here with no frontend change.
  it("groups the providers the table names, running-on-this-machine first", async () => {
    const { container } = await openAiSection();
    const groups = [...container.querySelectorAll('[data-setting="ai_provider"] optgroup')];
    expect(groups.map((g) => g.getAttribute("label"))).toEqual([
      "Running on this machine",
      "Hosted",
      "Custom (OpenAI-compatible)",
    ]);
    expect([...groups[0].querySelectorAll("option")].map((o) => o.textContent)).toEqual([
      "Ollama",
      "LM Studio",
    ]);
  });

  // The table arrives after the panel mounts, so the picker has no options when
  // the saved provider is first applied to it.
  it("shows the saved provider once the table arrives", async () => {
    const [groups, setGroups] = createSignal<ProviderGroupedOptions[]>([]);
    const grouped = vi.spyOn(aiProvidersStore, "grouped").mockImplementation(() => groups());
    try {
      const { container } = await openAiSection();
      const select = container.querySelector('[data-setting="ai_provider"]') as HTMLSelectElement;
      expect(select.querySelectorAll("option").length).toBe(0);

      setGroups(groupProviders(TEST_PROVIDERS as AiProviderInfo[]));
      await waitFor(() => expect(select.querySelectorAll("option").length).toBeGreaterThan(0));
      expect(select.value).toBe("deepseek");
    } finally {
      grouped.mockRestore();
    }
  });

  it("shows the base URL row only for a custom server", async () => {
    const hosted = await openAiSection();
    expect(hosted.container.querySelector('[data-setting-id="ai.base_url"]')).toBeNull();
    closeSettings();
    cleanup();
    const custom = await openAiSection({ provider: "custom", model: "" });
    expect(custom.container.querySelector('[data-setting-id="ai.base_url"]')).not.toBeNull();
  });

  it("hides the key row for a provider on this machine", async () => {
    const { container } = await openAiSection({ provider: "ollama", model: "" });
    expect(container.querySelector('[data-setting-id="ai.api_key"]')).toBeNull();
  });

  it("offers the provider's own key page", async () => {
    const { container } = await openAiSection();
    const link = container.querySelector('[data-action="ai-key-page"]')!;
    fireEvent.click(link);
    await waitFor(() =>
      expect(mocks.openExternalUrl).toHaveBeenCalledWith("https://platform.deepseek.com/api_keys"),
    );
  });

  it("says whether the local runtime answered its port", async () => {
    mocks.aiProbeLocal.mockResolvedValue({ ollama: true, lmstudio: false });
    const { container } = await openAiSection({ provider: "ollama", model: "" });
    await waitFor(() => expect(container.querySelector(".settings-ai-pill")).not.toBeNull());
    expect(container.querySelector(".settings-ai-pill")!.textContent).toBe("Running");
  });

  // The probe cannot tell "installed but stopped" from "not installed", so the
  // line states the fact it knows and the link covers the other case.
  it("explains a runtime that did not answer, and offers the download", async () => {
    mocks.aiProbeLocal.mockResolvedValue({ ollama: false, lmstudio: false });
    const { container } = await openAiSection({ provider: "ollama", model: "" });
    await waitFor(() => expect(container.querySelector(".settings-ai-pill")).not.toBeNull());
    expect(container.querySelector(".settings-ai-pill")!.textContent).toBe("Not running");
    expect(container.textContent).toContain("Writ could not reach Ollama on port 11434.");
    fireEvent.click(container.querySelector('[data-action="get-ollama"]')!);
    await waitFor(() =>
      expect(mocks.openExternalUrl).toHaveBeenCalledWith("https://ollama.com/download"),
    );
  });

  it("marks the curated ids as suggestions when no list was fetched", async () => {
    const { container } = await openAiSection();
    await waitFor(() =>
      expect(container.querySelector('[data-setting="ai_model"] option')).not.toBeNull(),
    );
    const options = [...container.querySelectorAll('[data-setting="ai_model"] option')];
    expect(options[0].textContent).toBe("deepseek-chat (suggested)");
    expect(options[options.length - 1].textContent).toBe("Custom…");
  });

  it("drops the suggestion marking once the provider answers with a list", async () => {
    mocks.aiListModels.mockResolvedValue(
      listed("deepseek", ["deepseek-chat", "deepseek-reasoner"]),
    );
    const { container } = await openAiSection();
    await waitFor(() =>
      expect(container.querySelector('[data-setting="ai_model"] option')!.textContent).toBe(
        "deepseek-chat",
      ),
    );
  });

  // The override is qualified by the provider it was picked under, and a
  // provider change goes through the one command that drops a foreign one. A
  // chat still sending an Ollama id to DeepSeek is the 400 this closes.
  it("switching provider clears a chat model that belonged to the old one", async () => {
    const { container } = await openAiSection({
      provider: "ollama",
      model: "qwen3:4b",
      chat: { enabled: true, model: "qwen2.5-coder:0.5b", model_provider: "ollama" },
    });
    const select = container.querySelector('[data-setting="ai_provider"]') as HTMLSelectElement;
    select.value = "deepseek";
    select.dispatchEvent(new Event("change", { bubbles: true }));

    await waitFor(() => expect(mocks.aiSetProvider).toHaveBeenCalledWith("deepseek"));
    // The panel does not write the connection itself: one command holds the
    // rule, so the pane and this section cannot clear an override differently.
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("names the host that notes and text would be sent to", async () => {
    const { container } = await openAiSection();
    await waitFor(() => expect(container.querySelector(".settings-ai-consent")).not.toBeNull());
    expect(container.querySelector(".settings-ai-consent-text")!.textContent).toContain(
      "api.deepseek.com",
    );
  });

  // The sentence is what a person reads before they press Allow, so it is
  // pinned word for word rather than by the host it names.
  it("states what a send carries, in the words the record settled on", async () => {
    const { container } = await openAiSection();
    await waitFor(() => expect(container.querySelector(".settings-ai-consent")).not.toBeNull());
    expect(collapse(container.querySelector(".settings-ai-consent-text")!.textContent)).toBe(
      "The notes you attach and the text you rewrite are sent to api.deepseek.com with your " +
        "API key. Writ also sends the key on its own to check the host is reachable; nothing " +
        "else leaves your machine.",
    );
    expect(container.querySelector('[data-action="ai-consent"]')!.textContent).toBe("Allow");
  });

  it("sits above the model and key rows, not below the connection line", async () => {
    const { container } = await openAiSection();
    await waitFor(() => expect(container.querySelector(".settings-ai-consent")).not.toBeNull());
    const notice = container.querySelector(".settings-ai-consent")!;
    const keyRow = container.querySelector('[data-setting-id="ai.api_key"]')!;
    // Buried below the fold is why the operator never saw it.
    expect(notice.compareDocumentPosition(keyRow) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("records consent host-side rather than patching the config itself", async () => {
    const { container, getByText } = await openAiSection();
    await waitFor(() => expect(container.querySelector(".settings-ai-consent")).not.toBeNull());
    fireEvent.click(getByText("Allow"));
    await waitFor(() => expect(mocks.aiConsentHost).toHaveBeenCalledTimes(1));
  });

  it("stays hidden once the host is consented to", async () => {
    mocks.aiEndpointState.mockResolvedValue(hostedEndpoint({ is_consented: true }));
    const { container } = await openAiSection();
    await waitFor(() => expect(mocks.aiEndpointState).toHaveBeenCalled());
    expect(container.querySelector(".settings-ai-consent")).toBeNull();
  });

  // A local row asks for nothing, so the same slot states what it does instead
  // of leaving a person to infer it.
  it("says nothing leaves the machine for a local provider, and asks for no consent", async () => {
    mocks.aiEndpointState.mockResolvedValue(
      hostedEndpoint({
        host: "localhost",
        host_port: "localhost:11434",
        is_hosted: false,
        is_consented: true,
        provider: "ollama",
      }),
    );
    const { container } = await openAiSection({ provider: "ollama", model: "" });
    await waitFor(() =>
      expect(container.querySelector('[data-note="local-endpoint"]')).not.toBeNull(),
    );
    expect(collapse(container.querySelector('[data-note="local-endpoint"]')!.textContent)).toBe(
      "Requests go to localhost:11434 on this machine. Nothing leaves it.",
    );
    expect(container.querySelector(".settings-ai-consent")).toBeNull();
  });

  // The check carries the API key, so an unconsented hosted endpoint is not
  // contacted at all. The line has to read as "nothing was sent yet", not as a
  // dead endpoint.
  it("reports a check held back for consent instead of a connection failure", async () => {
    mocks.aiCheckConnection.mockResolvedValue({
      reachable: false,
      model_listed: null,
      kind: "consent_required",
      detail: "api.deepseek.com",
      models: [],
    });
    const { container } = await openAiSection();
    await waitFor(() => {
      const line = container.querySelector(".settings-ai-connection-status");
      expect(line?.textContent).toBe("Not checked until you allow api.deepseek.com");
    });
    expect(
      container.querySelector(".settings-ai-connection-status")!.getAttribute("data-tone"),
    ).toBe("warn");
  });

  it("closing the chat model disclosure hands chat back to the connection's model", async () => {
    const { container } = await openAiSection({ chat: { enabled: true, model: "deepseek-reasoner", model_provider: "" } });
    const row = container.querySelector('[data-setting-id="ai.chat.model"]')!;
    expect(row.querySelector('[data-setting="ai_chat_model"]')).not.toBeNull();
    fireEvent.click(row.querySelector('[data-setting="ai_chat_model_disclosure"]')!);
    await waitFor(() => expect(mocks.save).toHaveBeenCalled());
    const calls = mocks.save.mock.calls;
    const saved = calls[calls.length - 1][0];
    expect(saved.ai.chat.model).toBe("");
  });
});

// The Notes section is the answer to "where are my notes" (ADR-028 §2), so it
// leads the nav rail and its row carries the path plus the three actions.
describe("Notes section", () => {
  beforeEach(() => {
    mocks.config.mockReset().mockReturnValue(baseConfig());
    mocks.notesFolder
      .mockReset()
      .mockReturnValue({
      path: "/home/user/Writ",
      display_path: "~/Writ",
      fallback: null,
      sync_provider: null,
    });
    mocks.notesLoadFolder.mockReset().mockResolvedValue(undefined);
    mocks.notesShowInFileManager.mockReset().mockResolvedValue(undefined);
    mocks.notesCopyPath.mockReset().mockResolvedValue(undefined);
    mocks.notesMove.mockReset().mockResolvedValue(null);
  });

  afterEach(() => {
    closeSettings();
    cleanup();
  });

  async function openNotes() {
    const result = render(() => <SettingsModal />);
    openSettings("notes");
    await waitFor(() =>
      expect(result.container.querySelector("[data-setting-id='notes.folder']")).not.toBeNull(),
    );
    return result;
  }

  it("leads the nav rail", async () => {
    const { container } = await openNotes();
    const first = container.querySelector(".settings-nav-item");
    expect(first?.textContent).toBe("Notes");
  });

  it("shows the folder path with the home folder collapsed", async () => {
    const { container } = await openNotes();
    const path = container.querySelector("[data-notes-path]");
    expect(path?.textContent).toBe("~/Writ");
  });

  it("fires the folder actions", async () => {
    const { container } = await openNotes();
    fireEvent.click(container.querySelector("[data-action='notes-show']")!);
    fireEvent.click(container.querySelector("[data-action='notes-copy']")!);
    fireEvent.click(container.querySelector("[data-action='notes-move']")!);
    await waitFor(() => {
      expect(mocks.notesShowInFileManager).toHaveBeenCalledTimes(1);
      expect(mocks.notesCopyPath).toHaveBeenCalledTimes(1);
      expect(mocks.notesMove).toHaveBeenCalledTimes(1);
    });
  });

  // ADR-028 §2: a `writ.db` path is never the answer to "where are my notes".
  it("shows the notes root and never a database path", async () => {
    const { container } = await openNotes();
    const row = container.querySelector("[data-setting-id='notes.folder']")!;
    expect(row.querySelector("[data-notes-path]")!.textContent).toBe("~/Writ");
    expect(row.textContent).not.toMatch(/\.db\b/);
    expect(row.querySelector("[data-storage-path]")).toBeNull();
  });

  it("says nothing about a fallback on an ordinary launch", async () => {
    const { container } = await openNotes();
    expect(container.querySelector("[data-notes-fallback]")).toBeNull();
  });

  it("names where the notes went when the folder in the settings was refused", async () => {
    mocks.notesFolder.mockReturnValue({
      path: "/home/user/Writ",
      display_path: "~/Writ",
      fallback: { from: "/volumes/gone/Notes", reason: "unusable" },
    });
    const { container } = await openNotes();
    expect(container.querySelector("[data-notes-fallback]")?.textContent).toBe(
      "The folder in your settings could not be used, so notes are in ~/Writ.",
    );
  });

  it("says which folder holds Writ's own data", async () => {
    mocks.notesFolder.mockReturnValue({
      path: "/home/user/Writ",
      display_path: "~/Writ",
      fallback: { from: "/home/user/.writ/archive", reason: "holds_writ_data" },
    });
    const { container } = await openNotes();
    expect(container.querySelector("[data-notes-fallback]")?.textContent).toBe(
      "The folder in your settings holds Writ's own data, so notes are in ~/Writ.",
    );
  });

  it("names the service syncing the folder", async () => {
    mocks.notesFolder.mockReturnValue({
      path: "/home/user/Library/Mobile Documents/com~apple~CloudDocs/Writ",
      display_path: "~/Library/Mobile Documents/com~apple~CloudDocs/Writ",
      fallback: null,
      sync_provider: "iCloud Drive",
    });
    const { container } = await openNotes();
    expect(container.querySelector("[data-notes-sync]")?.textContent).toBe(
      "iCloud Drive syncs this folder. Use one sync service per folder.",
    );
  });

  it("says how to sync a folder that nothing syncs", async () => {
    const { container } = await openNotes();
    expect(container.querySelector("[data-notes-sync]")?.textContent).toBe(
      "Writ has no sync. Put the notes folder in iCloud Drive, Dropbox, or Google Drive and your notes sync with it. Use one sync service per folder.",
    );
  });
});

const SERVER_READ_TOOLS = [
  "list_notes",
  "search_notes",
  "read_note",
  "note_links",
  "note_backlinks",
  "note_properties",
  "note_tags",
  "folder_tags",
];
const SERVER_WRITE_TOOLS = ["write_note", "create_note", "rename_note"];

const WRITING_CLIENT = {
  name: "Scribe CLI",
  first_seen: "2026-09-11T09:12:00Z",
  read: true,
  write: true,
};
const READING_CLIENT = {
  name: "Desk helper",
  first_seen: "2026-09-12T10:04:00Z",
  read: true,
  write: false,
};

const SETTINGS_CSS = readFileSync(
  resolve(process.cwd(), "src/components/SettingsModal/SettingsModal.css"),
  "utf8",
);

/** The real sheet in the document, so a row's widths can be read back. */
function injectSettingsCss(): HTMLStyleElement {
  const style = document.createElement("style");
  style.textContent = SETTINGS_CSS;
  document.head.append(style);
  return style;
}

async function openPrograms() {
  const result = render(() => <SettingsModal />);
  openSettings("programs");
  await waitFor(() =>
    expect(result.container.querySelector("[data-setting-id='mcp.tools']")).not.toBeNull(),
  );
  return result;
}

describe("Connected programs section — the tool row", () => {
  let sheet: HTMLStyleElement | undefined;

  beforeEach(() => {
    mocks.config.mockReset().mockReturnValue(baseConfig());
    mocks.mcpClients.mockReset().mockResolvedValue({ approved: [], waiting: [] });
    mocks.mcpServerCommand
      .mockReset()
      .mockResolvedValue({ path: "/usr/local/bin/writ", command: "writ mcp" });
    mocks.mcpTools.mockReset().mockResolvedValue({
      read: SERVER_READ_TOOLS,
      write: SERVER_WRITE_TOOLS,
    });
  });

  afterEach(() => {
    sheet?.remove();
    sheet = undefined;
    closeSettings();
    cleanup();
  });

  it("says in plain words what each grant lets a program do", async () => {
    const { container } = await openPrograms();

    await waitFor(() => expect(container.querySelector(".settings-tools")).not.toBeNull());
    expect(container.querySelector("[data-grant='read']")?.textContent).toBe(
      "Reading: list, search and open notes, and see their links, properties and tags.",
    );
    expect(container.querySelector("[data-grant='write']")?.textContent).toBe(
      "Writing: replace a note's text, make a new note, rename a note.",
    );
  });

  it("never lets the label column end up narrower than the list column", async () => {
    mocks.mcpClients.mockResolvedValue({ approved: [READING_CLIENT], waiting: [] });
    sheet = injectSettingsCss();
    const { container } = await openPrograms();

    await waitFor(() => expect(container.querySelector(".settings-programs")).not.toBeNull());
    for (const id of ["mcp.tools", "mcp.clients"]) {
      const row = container.querySelector<HTMLElement>(`[data-setting-id='${id}']`);
      expect(row, id).not.toBeNull();
      expect(row!.getAttribute("data-align")).toBe("start");
      expect(getComputedStyle(row!).alignItems).toBe("flex-start");

      const label = container.querySelector<HTMLElement>(`[data-setting-id='${id}'] .settings-row-label`);
      const list = container.querySelector<HTMLElement>(`[data-setting-id='${id}'] ul`);
      expect(label, id).not.toBeNull();
      expect(list, id).not.toBeNull();
      const labelStyle = getComputedStyle(label!);
      const listStyle = getComputedStyle(list!);
      expect(labelStyle.flexGrow).toBe("1");
      expect(listStyle.flexGrow).toBe("1");
      expect(parseFloat(labelStyle.flexBasis)).toBe(0);
      expect(parseFloat(listStyle.flexBasis)).toBe(0);
    }

    const programRow = container.querySelector<HTMLElement>(".settings-program-row");
    const grants = container.querySelector<HTMLElement>(".settings-program-grants");
    expect(getComputedStyle(programRow!).flexWrap).toBe("wrap");
    expect(getComputedStyle(grants!).flexWrap).toBe("wrap");
  });
});

describe("Connected programs section, programs you approved", () => {
  beforeEach(() => {
    mocks.config.mockReset().mockReturnValue(baseConfig());
    mocks.mcpClients.mockReset().mockResolvedValue({ approved: [], waiting: [] });
    mocks.mcpSetClientPermission.mockReset().mockResolvedValue({ approved: [], waiting: [] });
    mocks.mcpServerCommand
      .mockReset()
      .mockResolvedValue({ path: "/usr/local/bin/writ", command: "writ mcp" });
    mocks.mcpTools.mockReset().mockResolvedValue({
      read: SERVER_READ_TOOLS,
      write: SERVER_WRITE_TOOLS,
    });
  });

  afterEach(() => {
    closeSettings();
    cleanup();
  });

  async function openProgramList(approved: object[]) {
    mocks.mcpClients.mockResolvedValue({ approved, waiting: [] });
    const result = await openPrograms();
    await waitFor(() => expect(result.container.querySelector(".settings-program")).not.toBeNull());
    return result;
  }

  it("holds reading on and out of reach while a program may write", async () => {
    const { container } = await openProgramList([WRITING_CLIENT]);

    const read = container.querySelector<HTMLButtonElement>(
      '[data-setting="mcp_read_Scribe CLI"]',
    );
    expect(read?.getAttribute("aria-checked")).toBe("true");
    expect(read?.disabled).toBe(true);
    expect(
      container.querySelector("[data-program='Scribe CLI'] [data-program-note='write']")?.textContent,
    ).toBe("Writing includes reading.");

    read!.click();
    expect(mocks.mcpSetClientPermission).not.toHaveBeenCalled();
  });

  it("lets a reading program be switched either way", async () => {
    const { container } = await openProgramList([READING_CLIENT]);

    const read = container.querySelector<HTMLButtonElement>(
      '[data-setting="mcp_read_Desk helper"]',
    );
    const write = container.querySelector<HTMLButtonElement>(
      '[data-setting="mcp_write_Desk helper"]',
    );
    expect(read?.disabled).toBe(false);
    expect(
      container.querySelector("[data-program='Desk helper'] [data-program-note='write']"),
    ).toBeNull();

    fireEvent.click(write!);
    expect(mocks.mcpSetClientPermission).toHaveBeenCalledWith("Desk helper", true, true);

    mocks.mcpSetClientPermission.mockClear();
    fireEvent.click(read!);
    expect(mocks.mcpSetClientPermission).toHaveBeenCalledWith("Desk helper", false, false);
  });

  it("says what forgetting a program does, on every program", async () => {
    const { container } = await openProgramList([WRITING_CLIENT, READING_CLIENT]);

    const notes = container.querySelectorAll("[data-program-note='forget']");
    expect(notes.length).toBe(2);
    for (const note of notes) {
      expect(note.textContent).toBe(
        "Removes this program. It can ask again next time it connects.",
      );
    }
  });

  it("carries no caution on the row, now that the rule is at the switch", async () => {
    const { container } = await openProgramList([WRITING_CLIENT]);

    expect(
      container.querySelector("[data-setting-id='mcp.clients'] .settings-row-caution"),
    ).toBeNull();
  });
});
