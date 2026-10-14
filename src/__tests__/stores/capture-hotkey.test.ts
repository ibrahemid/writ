import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot } from "solid-js";

const h = vi.hoisted(() => ({
  captureHotkeyStatus: vi.fn(),
  setCaptureHotkey: vi.fn(),
  onEvent: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  captureHotkeyStatus: h.captureHotkeyStatus,
  setCaptureHotkey: h.setCaptureHotkey,
}));
vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));

import { createCaptureHotkeyStore } from "../../stores/global/hotkey";

function store() {
  return createRoot(() => createCaptureHotkeyStore());
}

describe("the capture chord's store", () => {
  beforeEach(() => {
    h.captureHotkeyStatus.mockReset();
    h.setCaptureHotkey.mockReset();
    h.onEvent.mockReset();
  });

  it("listens on its own event, apart from the window chord's", async () => {
    h.onEvent.mockResolvedValue(() => {});
    await store().subscribe();
    expect(h.onEvent).toHaveBeenCalledWith("hotkey:capture-status", expect.any(Function));
  });

  it("reads a capture chord another app holds as taken", async () => {
    h.captureHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Alt+Space",
      registered: false,
      unsupported: false,
    });
    const capture = store();
    await capture.load();
    expect(capture.isTaken()).toBe(true);
  });

  it("reads capture turned off as off, not taken", async () => {
    h.captureHotkeyStatus.mockResolvedValue({ chord: "", registered: false, unsupported: false });
    const capture = store();
    await capture.load();
    expect(capture.chord()).toBe("");
    expect(capture.isTaken()).toBe(false);
  });

  it("asks for the capture chord through its own call", async () => {
    h.setCaptureHotkey.mockResolvedValue({
      status: { chord: "Ctrl+Alt+N", registered: true, unsupported: false },
      refused: false,
    });
    const capture = store();
    expect(await capture.rebind("Ctrl+Alt+N")).toBe(true);
    expect(h.setCaptureHotkey).toHaveBeenCalledWith("Ctrl+Alt+N");
    expect(capture.chord()).toBe("Ctrl+Alt+N");
  });
});
