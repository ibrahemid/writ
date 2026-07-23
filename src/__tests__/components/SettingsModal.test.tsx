import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";

const TEST_CLAIMABLE_TYPE = {
  id: "markdown",
  label: "Markdown",
  exts: ["md", "markdown"],
  utis: ["net.daringfireball.markdown"],
};

const mocks = vi.hoisted(() => ({
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
}));

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn().mockResolvedValue(undefined),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  searchBuffers: vi.fn().mockResolvedValue([]),
  aiHasApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
  aiSetApiKey: vi.fn().mockResolvedValue({ is_set: true, memory_only: false }),
  aiClearApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
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

vi.mock("../../stores/global/storage", () => ({
  fetchStorageInfo: mocks.fetchStorageInfo,
  revealStoragePath: mocks.revealStoragePath,
  copyStoragePath: mocks.copyStoragePath,
}));

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: mocks.writeClipboardText,
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: mocks.focusEditor },
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
    setPreset: vi.fn(),
  },
}));

vi.spyOn(configStore, "save").mockImplementation(mocks.save);
vi.spyOn(configStore, "config").mockImplementation(mocks.config);

import SettingsModal, { openSettings, closeSettings } from "../../components/SettingsModal/SettingsModal";
import { SETTINGS_INDEX, SECTION_ORDER } from "../../settings";
import { clearDefaultAppSupport, probeDefaultAppSupport } from "../../stores/global/default-app-support";

function baseConfig(): WritConfig {
  return {
    hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
    sidebar: { toggle: "CmdOrCtrl+S", default_visible: false, position: "left", open: false },
    editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_typography: true, markdown_editing: true },
    window: { width: 1100, height: 720 },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "warp-dark", overrides: {} },
    commands: { usage: {} },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: { enabled: false, preset: "ollama", base_url: "http://localhost:11434/v1", model: "", consented_hosted: false },
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

  it("shows all 8 section nav items", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll(".settings-nav-item");
    expect(navItems.length).toBe(8);
  });

  it("shows Editor section by default", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => {
      expect(container.querySelector("[data-section='editor']")).not.toBeNull();
    });
  });

  it("switches to Files section on nav click", async () => {
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
    openSettings();
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
    openSettings();
    await waitFor(() => expect(container.querySelector("[data-section='editor']")).not.toBeNull());
    const fontSizeInput = container.querySelector<HTMLInputElement>("[data-setting='font_size']");
    fireEvent.change(fontSizeInput!, { target: { value: "200" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.font_size).toBeLessThanOrEqual(72);
  });

  it("saves word wrap toggle", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
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
    openSettings();
    await waitFor(() => expect(container.querySelector("[data-section='editor']")).not.toBeNull());
    const tabSizeInput = container.querySelector<HTMLInputElement>("[data-setting='tab_size']");
    expect(tabSizeInput).not.toBeNull();
    fireEvent.change(tabSizeInput!, { target: { value: "4" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.tab_size).toBe(4);
  });

  it("saves autosave delay from Files section", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
    const filesNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("files"));
    fireEvent.click(filesNav!);
    await waitFor(() => expect(container.querySelector("[data-section='files']")).not.toBeNull());
    const autosaveInput = container.querySelector<HTMLInputElement>("[data-setting='autosave_debounce_ms']");
    expect(autosaveInput).not.toBeNull();
    fireEvent.change(autosaveInput!, { target: { value: "500" } });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    const saved = mocks.save.mock.calls[0][0] as WritConfig;
    expect(saved.editor.autosave_debounce_ms).toBe(500);
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

  describe("Files section — default app rows", () => {
    async function openFilesSection(container: Element) {
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
      const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
      const filesNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("files"));
      fireEvent.click(filesNav!);
      await waitFor(() => expect(container.querySelector("[data-section='files']")).not.toBeNull());
    }

    it("hides default-app rows when status is unsupported", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "unsupported" });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      // Rows are hidden via <Show> when status is unsupported
      const makeDefaultBtns = container.querySelectorAll("[data-action^='make-default-']");
      expect(makeDefaultBtns.length).toBe(0);
    });

    it("shows 'Writ is the default' and hides Make default button when is_default", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "is_default" });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => {
        const status = container.querySelector(".settings-default-app-status-active");
        expect(status).not.toBeNull();
        expect(status!.textContent).toContain("Writ is the default");
      });
      const makeDefaultBtn = container.querySelector("[data-action='make-default-markdown']");
      expect(makeDefaultBtn).toBeNull();
    });

    it("shows other app name and Make default button when other_app with name", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: "TextEdit" });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => {
        const status = container.querySelector(".settings-default-app-status");
        expect(status).not.toBeNull();
        expect(status!.textContent).toContain("TextEdit is the default");
      });
      const makeDefaultBtn = container.querySelector("[data-action='make-default-markdown']");
      expect(makeDefaultBtn).not.toBeNull();
    });

    it("shows generic label and Make default button when other_app without name", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: null });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => {
        const status = container.querySelector(".settings-default-app-status");
        expect(status).not.toBeNull();
        expect(status!.textContent).toContain("Another app is the default");
      });
    });

    it("shows 'No default set' and Make default button when no_handler", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "no_handler" });
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => {
        const status = container.querySelector(".settings-default-app-status");
        expect(status).not.toBeNull();
        expect(status!.textContent).toContain("No default set");
      });
      const makeDefaultBtn = container.querySelector("[data-action='make-default-markdown']");
      expect(makeDefaultBtn).not.toBeNull();
    });

    it("calls claimDefaultApp on Make default click", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: "TextEdit" });
      mocks.claimDefaultApp.mockResolvedValue(undefined);
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => expect(container.querySelector("[data-action='make-default-markdown']")).not.toBeNull());
      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='make-default-markdown']")!);
      await waitFor(() => expect(mocks.claimDefaultApp).toHaveBeenCalledWith("markdown"));
    });

    it("re-queries status after 800ms delay and reflects updated handler", async () => {
      vi.useFakeTimers();
      // Initial load returns other_app for both rows (md + html each call once on mount).
      // After "Make default" + 800ms, the md row re-queries and gets is_default.
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: "TextEdit" });
      mocks.claimDefaultApp.mockResolvedValue(undefined);
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => expect(container.querySelector("[data-action='make-default-markdown']")).not.toBeNull());

      // Record call count after initial mount (2 rows × 1 call each = 2)
      const callsBeforeClick = mocks.fetchDefaultAppStatus.mock.calls.length;

      // Queue is_default for the next call (the re-query after 800ms)
      mocks.fetchDefaultAppStatus.mockResolvedValueOnce({ status: "is_default" });

      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='make-default-markdown']")!);
      await waitFor(() => expect(mocks.claimDefaultApp).toHaveBeenCalledWith("markdown"));

      // Timer has not fired yet — no additional fetch calls
      expect(mocks.fetchDefaultAppStatus).toHaveBeenCalledTimes(callsBeforeClick);

      await vi.runAllTimersAsync();

      // After the timer fires, fetchDefaultAppStatus is called once more and the
      // row should flip to "Writ is the default" with the Make default button gone.
      await waitFor(() => {
        expect(mocks.fetchDefaultAppStatus).toHaveBeenCalledTimes(callsBeforeClick + 1);
        expect(container.querySelector(".settings-default-app-status-active")).not.toBeNull();
        expect(container.querySelector("[data-action='make-default-markdown']")).toBeNull();
      });

      vi.useRealTimers();
    });
  });

  describe("Storage section", () => {
    async function openStorageSection(container: Element) {
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
      const navItems = container.querySelectorAll<HTMLButtonElement>(".settings-nav-item");
      const storageNav = Array.from(navItems).find((n) => n.textContent?.toLowerCase().includes("storage"));
      expect(storageNav).toBeTruthy();
      fireEvent.click(storageNav!);
      await waitFor(() => expect(container.querySelector("[data-section='storage']")).not.toBeNull());
    }

    it("shows the database path", async () => {
      const { container } = render(() => <SettingsModal />);
      await openStorageSection(container);
      await waitFor(() => {
        const path = container.querySelector("[data-storage-path]");
        expect(path).not.toBeNull();
        expect(path!.textContent).toContain("/home/user/.writ/writ.db");
      });
    });

    it("copies the path on Copy click", async () => {
      const { container } = render(() => <SettingsModal />);
      await openStorageSection(container);
      await waitFor(() =>
        expect(container.querySelector("[data-storage-path]")!.textContent).toContain("writ.db"),
      );
      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='storage-copy']")!);
      await waitFor(() =>
        expect(mocks.copyStoragePath).toHaveBeenCalledWith("/home/user/.writ/writ.db"),
      );
    });

    it("reveals the path on Reveal click", async () => {
      const { container } = render(() => <SettingsModal />);
      await openStorageSection(container);
      await waitFor(() =>
        expect(container.querySelector("[data-action='storage-reveal']")).not.toBeNull(),
      );
      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='storage-reveal']")!);
      await waitFor(() => expect(mocks.revealStoragePath).toHaveBeenCalledTimes(1));
    });

    it("surfaces the storage location in search by keyword", async () => {
      const { container } = render(() => <SettingsModal />);
      openSettings();
      await waitFor(() => expect(container.querySelector(".settings-search-input")).not.toBeNull());
      const input = container.querySelector<HTMLInputElement>(".settings-search-input")!;
      fireEvent.input(input, { target: { value: "database" } });
      await waitFor(() => {
        expect(container.querySelector("[data-setting-id='storage.location']")).not.toBeNull();
      });
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
      await openAndSearch(container, "font");
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
        expect(
          container.querySelector("[data-setting-id='files.default_app.config-data']"),
        ).not.toBeNull();
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
      // Render every claimable group the index knows about, all supported, so
      // the dynamic default-app rows are present for the parity comparison.
      mocks.fetchDefaultAppTypes.mockResolvedValue(
        ["plain-text", "markdown", "config-data", "source-code"].map((id) => ({
          id,
          label: id,
          exts: [id],
          utis: [`public.${id}`],
        })),
      );
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "is_default" });
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
