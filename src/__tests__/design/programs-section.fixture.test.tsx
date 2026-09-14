import { describe, it, expect, vi, afterEach } from "vitest";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
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
  ai: { provider: "ollama", base_url: "", model: "", consented_hosts: [], rewrite: { enabled: false }, chat: { enabled: false, model: "" } },
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


// The programs row is read at three interface text sizes and at the window's
// minimum width, so the design gate can see the rows it has to fit. Off unless
// the gate points WRIT_DESIGN_FIXTURE_DIR at a folder.

const FIXTURE_DIR = process.env.WRIT_DESIGN_FIXTURE_DIR;
const SIZES = [12, 16, 22];

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

const CLIENTS = [
  { name: "Scribe CLI", first_seen: "2026-09-11T09:12:00Z", read: true, write: true },
  { name: "Desk helper", first_seen: "2026-09-12T10:04:00Z", read: true, write: false },
];

const SHEETS = [
  "src/styles/generated/theme.css",
  "src/styles/global.css",
  "src/App.css",
  "src/components/Button/Button.css",
  "src/components/SettingsModal/SettingsModal.css",
];

function styleBlocks(): string {
  return SHEETS.filter((sheet) => existsSync(resolve(process.cwd(), sheet)))
    .map((sheet) => `<style data-sheet="${sheet}">\n${readFileSync(resolve(process.cwd(), sheet), "utf8")}\n</style>`)
    .join("\n");
}

function rootAttributes(): string {
  const root = document.documentElement;
  return [...root.attributes].map((attr) => `${attr.name}="${attr.value}"`).join(" ");
}

function page(size: number, modal: string, extra = ""): string {
  return `<!doctype html>
<html ${rootAttributes()}>
<head>
<meta charset="utf-8" />
${styleBlocks()}
<style>:root{--writ-ui-size:${size}px}</style>
${extra}
</head>
<body style="padding:24px">
${modal}
</body>
</html>
`;
}

async function renderPrograms(): Promise<string> {
  mocks.config.mockReset().mockReturnValue(baseConfig());
  mocks.polarity.mockReset().mockReturnValue("light");
  mocks.accentApplies.mockReset().mockReturnValue(true);
  mocks.activePresetId.mockReset().mockReturnValue("writ-light");
  mocks.mcpClients.mockReset().mockResolvedValue({ approved: CLIENTS, waiting: [] });
  mocks.mcpServerCommand
    .mockReset()
    .mockResolvedValue({ path: "/usr/local/bin/writ", command: "writ mcp" });
  mocks.mcpTools.mockReset().mockResolvedValue({
    read: SERVER_READ_TOOLS,
    write: SERVER_WRITE_TOOLS,
  });

  const { container } = render(() => <SettingsModal />);
  openSettings("programs");
  await waitFor(() =>
    expect(container.querySelector("[data-setting-id='mcp.clients'] .settings-program")).not.toBeNull(),
  );
  const modal = container.querySelector(".settings-modal");
  expect(modal).not.toBeNull();
  return modal!.outerHTML;
}

describe("programs section fixtures", () => {
  afterEach(() => {
    closeSettings();
    cleanup();
  });

  it.skipIf(!FIXTURE_DIR)(
    "writes the programs row at each interface text size",
    async () => {
      const dir = FIXTURE_DIR!;
      mkdirSync(dir, { recursive: true });
      // The theme store is mocked, so the attributes it would put on the root
      // for this polarity and accent are set here instead.
      document.documentElement.setAttribute("data-theme", "light");
      document.documentElement.setAttribute("data-accent", "pine");

      let widest = "";
      for (const size of SIZES) {
        const modal = await renderPrograms();
        writeFileSync(join(dir, `programs-${size}.html`), page(size, modal), "utf8");
        if (size === 22) widest = modal;
        cleanup();
        closeSettings();
      }

      writeFileSync(
        join(dir, "programs-22-narrow.html"),
        page(22, widest, "<style>.settings-modal{width:672px}</style>"),
        "utf8",
      );

      for (const name of ["programs-12", "programs-16", "programs-22", "programs-22-narrow"]) {
        expect(existsSync(join(dir, `${name}.html`)), name).toBe(true);
      }
    },
    30_000,
  );
});
