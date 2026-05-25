import { describe, it, expect, vi, afterEach, beforeEach, beforeAll } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
  saveConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ keybindings: {} }),
    save: h.saveConfig,
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

import ShortcutEditor, {
  openShortcutEditor,
  closeShortcutEditor,
} from "../../components/ShortcutEditor/ShortcutEditor";
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

describe("ShortcutEditor keyboard integration", () => {
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
    rebuildKeyMap();
  });

  afterEach(() => {
    closeShortcutEditor();
    cleanup();
    resetDom();
    h.focusEditor.mockClear();
  });

  async function setup() {
    const container = mountInAppShell();
    const user = userEvent.setup({ document });
    render(() => <ShortcutEditor />, { container });
    openShortcutEditor();
    await new Promise<void>((r) => setTimeout(r, 0));
    return user;
  }

  it("opens and focuses an element inside the dialog", async () => {
    await setup();
    const dialog = document.querySelector<HTMLElement>(".shortcut-editor")!;
    expect(dialog).not.toBeNull();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("does not mark #app as inert", async () => {
    await setup();
    expect(document.getElementById("app")!.hasAttribute("inert")).toBe(false);
  });

  it("Tab cycles inside the dialog", async () => {
    const user = await setup();
    const dialog = document.querySelector<HTMLElement>(".shortcut-editor")!;
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("Record button enters listening state", async () => {
    const user = await setup();
    const recordBtns = document.querySelectorAll<HTMLButtonElement>(".shortcut-row-btn");
    const recordBtn = Array.from(recordBtns).find((b) => b.textContent?.trim() === "Record");
    expect(recordBtn).toBeDefined();
    await user.click(recordBtn!);
    expect(document.querySelector(".shortcut-row-listening")).not.toBeNull();
  });

  it("Escape during recording cancels recording only, dialog stays open", async () => {
    const user = await setup();
    const recordBtns = document.querySelectorAll<HTMLButtonElement>(".shortcut-row-btn");
    const recordBtn = Array.from(recordBtns).find((b) => b.textContent?.trim() === "Record");
    await user.click(recordBtn!);
    expect(document.querySelector(".shortcut-row-listening")).not.toBeNull();

    await user.keyboard("{Escape}");
    expect(document.querySelector(".shortcut-row-listening")).toBeNull();
    expect(document.querySelector(".shortcut-editor")).not.toBeNull();
  });

  it("Escape when not recording closes the modal", async () => {
    const user = await setup();
    await user.keyboard("{Escape}");
    expect(document.querySelector(".shortcut-editor")).toBeNull();
  });
});
