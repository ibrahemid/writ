import { describe, it, expect, vi, afterEach, beforeEach, beforeAll } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
  saveConfig: vi.fn().mockResolvedValue(undefined),
  keybindings: {} as Record<string, string>,
}));

vi.mock("../../stores/global/config", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../stores/global/config")>()),
  configStore: {
    config: () => ({
      keybindings: h.keybindings,
      hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
      updater: { auto_check: true },
    }),
    save: h.saveConfig,
    isAppOn: () => false,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: h.focusEditor },
  }),
}));

vi.mock("../Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import SettingsModal, {
  closeSettings,
  openSettings,
} from "../../components/SettingsModal/SettingsModal";
import ConfirmDialog from "../../components/ConfirmDialog/ConfirmDialog";
import { registerCommand, getAllCommands, unregisterCommand } from "../../commands/registry";
import { rebuildKeyMap } from "../../commands/keybindings";

function resetDom() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
}

function mountInAppShell() {
  resetDom();
  const appRoot = document.createElement("div");
  appRoot.id = "app";
  document.body.appendChild(appRoot);
  return appRoot;
}

// The shortcut list as Settings, Shortcuts hosts it: Settings' own focus trap
// has to step aside while a row records a chord.
describe("the shortcut list inside Settings, by keyboard", () => {
  beforeEach(() => {
    for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
    registerCommand({
      id: "cmd.alpha",
      label: "Alpha command",
      keybinding: "CmdOrCtrl+A",
      scope: "app",
      execute: vi.fn(),
    });
    registerCommand({
      id: "cmd.bravo",
      label: "Bravo command",
      keybinding: "CmdOrCtrl+B",
      scope: "app",
      execute: vi.fn(),
    });
    h.keybindings = {};
    rebuildKeyMap();
  });

  afterEach(() => {
    closeSettings();
    cleanup();
    resetDom();
    h.focusEditor.mockClear();
  });

  async function setup() {
    const container = mountInAppShell();
    const user = userEvent.setup({ document });
    render(
      () => (
        <>
          <SettingsModal />
          <ConfirmDialog />
        </>
      ),
      { container },
    );
    openSettings("shortcuts");
    await new Promise<void>((r) => setTimeout(r, 0));
    return user;
  }

  it("opens Settings on the list and focuses inside the dialog", async () => {
    await setup();
    expect(document.querySelector(".settings-modal .shortcut-editor")).not.toBeNull();
    const dialog = document.querySelector<HTMLElement>(".settings-modal")!;
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("does not mark #app as inert", async () => {
    await setup();
    expect(document.getElementById("app")!.hasAttribute("inert")).toBe(false);
  });

  it("Tab cycles inside the dialog", async () => {
    const user = await setup();
    const dialog = document.querySelector<HTMLElement>(".settings-modal")!;
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("Record button enters listening state", async () => {
    const user = await setup();
    const recordBtns = document.querySelectorAll<HTMLButtonElement>("[data-action='record-shortcut']");
    const recordBtn = Array.from(recordBtns).find((b) => b.textContent?.trim() === "Record");
    expect(recordBtn).toBeDefined();
    await user.click(recordBtn!);
    expect(document.querySelector(".shortcut-row-listening")).not.toBeNull();
  });

  it("Escape during recording cancels recording only, Settings stays open", async () => {
    const user = await setup();
    const recordBtns = document.querySelectorAll<HTMLButtonElement>("[data-action='record-shortcut']");
    const recordBtn = Array.from(recordBtns).find((b) => b.textContent?.trim() === "Record");
    await user.click(recordBtn!);
    expect(document.querySelector(".shortcut-row-listening")).not.toBeNull();

    await user.keyboard("{Escape}");
    expect(document.querySelector(".shortcut-row-listening")).toBeNull();
    expect(document.querySelector(".settings-modal .shortcut-editor")).not.toBeNull();
  });

  it("Escape when not recording closes Settings", async () => {
    const user = await setup();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(document.querySelector(".settings-modal")).toBeNull());
  });

  it("asks before Escape throws a recorded row away, and keeps it on Cancel", async () => {
    h.keybindings = { "cmd.alpha": "CmdOrCtrl+Shift+A" };
    const user = await setup();
    await user.click(document.querySelector<HTMLButtonElement>("[data-action='reset-all-shortcuts']")!);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(document.querySelector(".confirm-dialog")).not.toBeNull());
    expect(document.body.textContent).toContain("Discard your changes?");
    await user.click(document.querySelector<HTMLButtonElement>(".confirm-cancel")!);
    await waitFor(() => expect(document.querySelector(".confirm-dialog")).toBeNull());
    expect(document.querySelector(".settings-modal .shortcut-editor")).not.toBeNull();
  });

  it("keeps a recorded row while another section is open", async () => {
    h.keybindings = { "cmd.alpha": "CmdOrCtrl+Shift+A" };
    const user = await setup();
    await user.click(document.querySelector<HTMLButtonElement>("[data-action='reset-all-shortcuts']")!);
    await user.click(document.querySelector<HTMLButtonElement>("[data-section-tab='about']")!);
    await waitFor(() => expect(document.querySelector(".shortcut-editor")).toBeNull());
    await user.click(document.querySelector<HTMLButtonElement>("[data-section-tab='shortcuts']")!);
    await waitFor(() => expect(document.querySelector(".shortcut-editor")).not.toBeNull());
    const alpha = Array.from(document.querySelectorAll(".shortcut-row")).find((row) =>
      row.textContent?.includes("Alpha command"),
    )!;
    expect(alpha.querySelector(".shortcut-row-changed")).toBeNull();
    expect(document.querySelector<HTMLButtonElement>("[data-action='save-shortcuts']")!.disabled).toBe(
      false,
    );
  });
});
