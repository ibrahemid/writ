import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";

const mocks = vi.hoisted(() => ({
  save: vi.fn().mockResolvedValue(undefined),
  config: vi.fn(),
  focusEditor: vi.fn(),
  openThemeEditor: vi.fn(),
  openShortcutEditor: vi.fn(),
  fetchDefaultAppStatus: vi.fn().mockResolvedValue({ status: "unsupported" }),
  claimDefaultApp: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn().mockResolvedValue(undefined),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  searchBuffers: vi.fn().mockResolvedValue([]),
}));

vi.mock("../../stores/global/default-app", () => ({
  fetchDefaultAppStatus: mocks.fetchDefaultAppStatus,
  claimDefaultApp: mocks.claimDefaultApp,
}));

vi.mock("../../stores/global/cli", () => ({
  installCli: vi.fn().mockResolvedValue({ symlink_path: "/usr/local/bin/writ", manual_command: "" }),
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

function baseConfig(): WritConfig {
  return {
    hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
    sidebar: { toggle: "CmdOrCtrl+S", default_visible: false, position: "left", open: false },
    editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_typography: true },
    window: { width: 1100, height: 720 },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "warp-dark", overrides: {} },
    commands: { usage: {} },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
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

  it("shows all 6 section nav items", async () => {
    const { container } = render(() => <SettingsModal />);
    openSettings();
    await waitFor(() => expect(container.querySelector(".settings-nav")).not.toBeNull());
    const navItems = container.querySelectorAll(".settings-nav-item");
    expect(navItems.length).toBe(6);
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
      const makeDefaultBtn = container.querySelector("[data-action='make-default-md']");
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
      const makeDefaultBtn = container.querySelector("[data-action='make-default-md']");
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
      const makeDefaultBtn = container.querySelector("[data-action='make-default-md']");
      expect(makeDefaultBtn).not.toBeNull();
    });

    it("calls claimDefaultApp on Make default click", async () => {
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: "TextEdit" });
      mocks.claimDefaultApp.mockResolvedValue(undefined);
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => expect(container.querySelector("[data-action='make-default-md']")).not.toBeNull());
      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='make-default-md']")!);
      await waitFor(() => expect(mocks.claimDefaultApp).toHaveBeenCalledWith("md"));
    });

    it("re-queries status after 800ms delay and reflects updated handler", async () => {
      vi.useFakeTimers();
      // Initial load returns other_app for both rows (md + html each call once on mount).
      // After "Make default" + 800ms, the md row re-queries and gets is_default.
      mocks.fetchDefaultAppStatus.mockResolvedValue({ status: "other_app", name: "TextEdit" });
      mocks.claimDefaultApp.mockResolvedValue(undefined);
      const { container } = render(() => <SettingsModal />);
      await openFilesSection(container);
      await waitFor(() => expect(container.querySelector("[data-action='make-default-md']")).not.toBeNull());

      // Record call count after initial mount (2 rows × 1 call each = 2)
      const callsBeforeClick = mocks.fetchDefaultAppStatus.mock.calls.length;

      // Queue is_default for the next call (the re-query after 800ms)
      mocks.fetchDefaultAppStatus.mockResolvedValueOnce({ status: "is_default" });

      fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='make-default-md']")!);
      await waitFor(() => expect(mocks.claimDefaultApp).toHaveBeenCalledWith("md"));

      // Timer has not fired yet — no additional fetch calls
      expect(mocks.fetchDefaultAppStatus).toHaveBeenCalledTimes(callsBeforeClick);

      await vi.runAllTimersAsync();

      // After the timer fires, fetchDefaultAppStatus is called once more and the
      // row should flip to "Writ is the default" with the Make default button gone.
      await waitFor(() => {
        expect(mocks.fetchDefaultAppStatus).toHaveBeenCalledTimes(callsBeforeClick + 1);
        expect(container.querySelector(".settings-default-app-status-active")).not.toBeNull();
        expect(container.querySelector("[data-action='make-default-md']")).toBeNull();
      });

      vi.useRealTimers();
    });
  });
});
