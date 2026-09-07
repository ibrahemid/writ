import { describe, it, expect, vi, beforeEach, afterEach, beforeAll } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
  saveConfig: vi.fn().mockResolvedValue(undefined),
  setGlobalHotkey: vi.fn(),
  globalHotkeyStatus: vi.fn(),
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ keybindings: {}, hotkey: { toggle: "CmdOrCtrl+Shift+Space" } }),
    save: h.saveConfig,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../services/tauri", () => ({
  setGlobalHotkey: h.setGlobalHotkey,
  globalHotkeyStatus: h.globalHotkeyStatus,
}));

vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));

import ShortcutEditor, {
  openShortcutEditor,
  closeShortcutEditor,
} from "../../components/ShortcutEditor/ShortcutEditor";
import { hotkeyStore } from "../../stores/global/hotkey";
import { getAllCommands, unregisterCommand } from "../../commands/registry";

function row(container: HTMLElement): HTMLElement {
  const found = container.querySelector<HTMLElement>('[data-shortcut="global-toggle"]');
  expect(found, "the shortcut editor carries a row for the window's own chord").not.toBeNull();
  return found!;
}

describe("the chord that shows and hides the window", () => {
  beforeEach(() => {
    for (const command of [...getAllCommands()]) unregisterCommand(command.id);
    h.setGlobalHotkey.mockReset();
    h.globalHotkeyStatus.mockReset();
  });

  afterEach(() => {
    closeShortcutEditor();
    cleanup();
  });

  it("shows the chord it holds, and no warning, while the OS gives it up", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    expect(row(container).textContent).toContain("Show and hide Writ");
    expect(row(container).querySelector('[data-state="taken"]')).toBeNull();
  });

  it("says the shortcut is taken, and offers a rebind, when another app holds it", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: false,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    const taken = row(container).querySelector('[data-state="taken"]');
    expect(taken?.textContent).toContain("Another app is using this shortcut");
    expect(
      row(container).querySelector('[data-action="record-global-shortcut"]'),
      "a taken chord is never a dead row: it offers the recorder",
    ).not.toBeNull();
  });

  it("records a new chord and asks the OS for it", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: false,
    });
    h.setGlobalHotkey.mockResolvedValue({ chord: "CmdOrCtrl+Alt+Space", registered: true });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
    fireEvent.keyDown(document, { key: "Space", metaKey: true, altKey: true });

    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await Promise.resolve();
    await Promise.resolve();

    expect(h.setGlobalHotkey).toHaveBeenCalledWith("CmdOrCtrl+Alt+Space");
    expect(hotkeyStore.isTaken()).toBe(false);
    expect(hotkeyStore.chord()).toBe("CmdOrCtrl+Alt+Space");
  });

  it("saves the chord the OS took, not the one that was asked for", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
    });
    // A chord the recorder can capture but the hotkey parser cannot read is
    // registered as the default, and that is what the config has to hold.
    h.setGlobalHotkey.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
    });
    h.saveConfig.mockClear();
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
    fireEvent.keyDown(document, { key: "F13", metaKey: true });

    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(h.setGlobalHotkey).toHaveBeenCalledWith("CmdOrCtrl+F13");
    expect(h.saveConfig).toHaveBeenCalledWith(
      expect.objectContaining({ hotkey: { toggle: "CmdOrCtrl+Shift+Space" } }),
    );
  });
});
