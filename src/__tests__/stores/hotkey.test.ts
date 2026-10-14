import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot } from "solid-js";

const h = vi.hoisted(() => ({
  globalHotkeyStatus: vi.fn(),
  setGlobalHotkey: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  globalHotkeyStatus: h.globalHotkeyStatus,
  setGlobalHotkey: h.setGlobalHotkey,
}));
vi.mock("../../services/events", () => ({ onEvent: vi.fn() }));

import { createHotkeyStore } from "../../stores/global/hotkey";

function store() {
  return createRoot(() => createHotkeyStore());
}

describe("the window chord's store", () => {
  beforeEach(() => {
    h.globalHotkeyStatus.mockReset();
    h.setGlobalHotkey.mockReset();
  });

  it("reads a refused rebind as not given, and keeps naming the chord still held", async () => {
    const hotkey = store();
    h.setGlobalHotkey.mockResolvedValue({
      status: { chord: "CmdOrCtrl+Shift+Space", registered: true, unsupported: false },
      refused: true,
    });

    expect(await hotkey.rebind("Ctrl+Alt+Space")).toBe(false);
    expect(hotkey.chord()).toBe("CmdOrCtrl+Shift+Space");
    expect(hotkey.isTaken()).toBe(false);
  });

  it("reads a given rebind as given", async () => {
    const hotkey = store();
    h.setGlobalHotkey.mockResolvedValue({
      status: { chord: "Ctrl+Alt+Space", registered: true, unsupported: false },
      refused: false,
    });

    expect(await hotkey.rebind("Ctrl+Alt+Space")).toBe(true);
    expect(hotkey.chord()).toBe("Ctrl+Alt+Space");
  });

  it("tells a taken chord from a session that cannot deliver one", async () => {
    const hotkey = store();
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: false,
      unsupported: false,
    });
    await hotkey.load();
    expect(hotkey.isTaken()).toBe(true);
    expect(hotkey.isUnsupported()).toBe(false);

    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: false,
      unsupported: true,
    });
    await hotkey.load();
    expect(hotkey.isTaken(), "Wayland is not another app holding the chord").toBe(false);
    expect(hotkey.isUnsupported()).toBe(true);
  });
});
