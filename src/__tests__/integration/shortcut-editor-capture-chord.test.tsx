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
  setCaptureHotkey: vi.fn(),
  captureHotkeyStatus: vi.fn(),
  onEvent: vi.fn().mockResolvedValue(() => {}),
  savedCapture: "CmdOrCtrl+Shift+Alt+Space",
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({
      keybindings: {},
      hotkey: { toggle: "CmdOrCtrl+Shift+Space", capture: h.savedCapture },
    }),
    save: h.saveConfig,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../services/tauri", () => ({
  setGlobalHotkey: h.setGlobalHotkey,
  globalHotkeyStatus: h.globalHotkeyStatus,
  setCaptureHotkey: h.setCaptureHotkey,
  captureHotkeyStatus: h.captureHotkeyStatus,
}));

vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));

import ShortcutEditor, {
  discardShortcutDrafts,
  windowChordRule,
} from "../../components/ShortcutEditor/ShortcutEditor";
import { detectPlatform } from "../../lib/platform";
import { captureHotkeyStore, hotkeyStore } from "../../stores/global/hotkey";
import { getAllCommands, unregisterCommand } from "../../commands/registry";

function captureRow(container: HTMLElement): HTMLElement {
  const found = container.querySelector<HTMLElement>('[data-shortcut="global-capture"]');
  expect(found, "the shortcut editor carries a row for the capture chord").not.toBeNull();
  return found!;
}

function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function status(chord: string, registered = true) {
  return { chord, registered, unsupported: false };
}

describe("the capture chord's row", () => {
  beforeEach(async () => {
    for (const command of [...getAllCommands()]) unregisterCommand(command.id);
    h.setGlobalHotkey.mockReset();
    h.setCaptureHotkey.mockReset();
    h.saveConfig.mockClear();
    h.globalHotkeyStatus.mockResolvedValue(status("CmdOrCtrl+Shift+Space"));
    h.captureHotkeyStatus.mockResolvedValue(status("CmdOrCtrl+Shift+Alt+Space"));
    await hotkeyStore.load();
    await captureHotkeyStore.load();
  });

  afterEach(() => {
    discardShortcutDrafts();
    cleanup();
  });

  it("sits beside the window's row and says when another app holds the chord", async () => {
    h.captureHotkeyStatus.mockResolvedValue(status("CmdOrCtrl+Shift+Alt+Space", false));
    await captureHotkeyStore.load();
    const { container } = render(() => <ShortcutEditor />);
    expect(container.querySelector('[data-shortcut="global-toggle"]')).not.toBeNull();
    expect(captureRow(container).querySelector('[data-state="taken"]')).not.toBeNull();
    expect(
      container.querySelector('[data-shortcut="global-toggle"] [data-state="taken"]'),
    ).toBeNull();
  });

  it("refuses a bare key with the rule and keeps listening", async () => {
    const { container } = render(() => <ShortcutEditor />);
    fireEvent.click(captureRow(container).querySelector('[data-action="record-capture-shortcut"]')!);
    fireEvent.keyDown(document, { key: "n" });
    expect(captureRow(container).textContent).toContain(windowChordRule(detectPlatform()));
    expect(captureRow(container).textContent).toContain("Press a key");
  });

  it("refuses the window's chord, which the OS can give to only one of them", async () => {
    const { container } = render(() => <ShortcutEditor />);
    fireEvent.click(captureRow(container).querySelector('[data-action="record-capture-shortcut"]')!);
    fireEvent.keyDown(document, { key: " ", metaKey: true, shiftKey: true });
    expect(captureRow(container).textContent).toContain("Show and hide Writ uses this shortcut.");
  });

  it("asks for a new capture chord on its own and records it", async () => {
    h.setCaptureHotkey.mockResolvedValue({ status: status("CmdOrCtrl+Alt+N"), refused: false });
    const { container } = render(() => <ShortcutEditor />);
    fireEvent.click(captureRow(container).querySelector('[data-action="record-capture-shortcut"]')!);
    fireEvent.keyDown(document, { key: "n", metaKey: true, altKey: true });
    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await settle();

    expect(h.setCaptureHotkey).toHaveBeenCalledWith("CmdOrCtrl+Alt+N");
    expect(h.setGlobalHotkey).not.toHaveBeenCalled();
    expect(h.saveConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        hotkey: { toggle: "CmdOrCtrl+Shift+Space", capture: "CmdOrCtrl+Alt+N" },
      }),
    );
  });

  it("keeps the old capture chord, and the window's, when another app holds the new one", async () => {
    h.setCaptureHotkey.mockResolvedValue({
      status: status("CmdOrCtrl+Shift+Alt+Space"),
      refused: true,
    });
    const { container } = render(() => <ShortcutEditor />);
    fireEvent.click(captureRow(container).querySelector('[data-action="record-capture-shortcut"]')!);
    fireEvent.keyDown(document, { key: "n", metaKey: true, altKey: true });
    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await settle();

    expect(captureRow(container).querySelector('[data-state="refused"]')).not.toBeNull();
    expect(h.saveConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        hotkey: { toggle: "CmdOrCtrl+Shift+Space", capture: "CmdOrCtrl+Shift+Alt+Space" },
      }),
    );
  });

  it("Reset all brings back the shipped capture chord", async () => {
    h.savedCapture = "Ctrl+Alt+K";
    h.captureHotkeyStatus.mockResolvedValue(status("Ctrl+Alt+K"));
    await captureHotkeyStore.load();
    h.setCaptureHotkey.mockResolvedValue({
      status: status("CmdOrCtrl+Shift+Alt+Space"),
      refused: false,
    });
    try {
      const { container } = render(() => <ShortcutEditor />);
        fireEvent.click(container.querySelector('[data-action="reset-all-shortcuts"]')!);
      fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
      await settle();
      expect(h.setCaptureHotkey).toHaveBeenCalledWith("CmdOrCtrl+Shift+Alt+Space");
      expect(h.saveConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          hotkey: { toggle: "CmdOrCtrl+Shift+Space", capture: "CmdOrCtrl+Shift+Alt+Space" },
        }),
      );
    } finally {
      h.savedCapture = "CmdOrCtrl+Shift+Alt+Space";
    }
  });
});
