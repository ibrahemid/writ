import { describe, it, expect, vi, beforeEach } from "vitest";

const h = vi.hoisted(() => ({
  capture: undefined as string | undefined,
  held: "",
}));

vi.mock("../../../stores/global/config", () => ({
  configStore: {
    config: () => ({ hotkey: { toggle: "CmdOrCtrl+Shift+Space", capture: h.capture } }),
  },
}));

vi.mock("../../../stores/global/hotkey", () => ({
  SHIPPED_CAPTURE_CHORD: "CmdOrCtrl+Shift+Alt+Space",
  captureHotkeyStore: { chord: () => h.held },
}));

import {
  heldCaptureChord,
  readAnyAppChord,
  savedCaptureChord,
  windowChordRule,
} from "../../../components/ShortcutEditor/any-app-chord";

const OTHER = { chord: "CmdOrCtrl+Shift+Alt+Space", label: "Quick capture" };

describe("reading a chord for a row the OS holds for every app", () => {
  it("states the rule for a bare key without resetting the recorder", () => {
    expect(readAnyAppChord({ kind: "bare", binding: "A" }, "mac", OTHER)).toEqual({
      kind: "refuse",
      text: windowChordRule("mac"),
      resetRecorder: false,
    });
  });

  it("waits while only modifiers are down", () => {
    expect(readAnyAppChord({ kind: "waiting" }, "win", OTHER)).toEqual({ kind: "wait" });
  });

  it("refuses Shift alone, which types a capital in every other app", () => {
    expect(readAnyAppChord({ kind: "captured", binding: "Shift+A" }, "linux", OTHER)).toEqual({
      kind: "refuse",
      text: windowChordRule("linux"),
      resetRecorder: true,
    });
  });

  it("takes F13 to F24 on their own", () => {
    expect(readAnyAppChord({ kind: "captured", binding: "F13" }, "win", OTHER)).toEqual({
      kind: "take",
      binding: "F13",
    });
  });

  it("refuses the other row's chord, whatever order its modifiers come in", () => {
    expect(
      readAnyAppChord({ kind: "captured", binding: "Alt+Shift+CmdOrCtrl+Space" }, "mac", OTHER),
    ).toEqual({ kind: "refuse", text: "Quick capture uses this shortcut.", resetRecorder: true });
  });

  it("takes a chord that holds Cmd, Ctrl or Alt", () => {
    expect(readAnyAppChord({ kind: "captured", binding: "CmdOrCtrl+Alt+K" }, "mac", OTHER)).toEqual(
      { kind: "take", binding: "CmdOrCtrl+Alt+K" },
    );
  });

  it("names the modifiers each platform has", () => {
    expect(windowChordRule("mac")).toContain("Command, Control or Option");
    expect(windowChordRule("win")).toContain("Ctrl or Alt");
  });
});

describe("the capture chord the window's row compares with", () => {
  beforeEach(() => {
    h.capture = undefined;
    h.held = "";
  });

  it("falls back to the shipped chord when none is saved", () => {
    expect(savedCaptureChord()).toBe("CmdOrCtrl+Shift+Alt+Space");
    expect(heldCaptureChord()).toBe("CmdOrCtrl+Shift+Alt+Space");
  });

  it("prefers the chord the OS holds over the saved one", () => {
    h.capture = "Ctrl+Alt+K";
    expect(heldCaptureChord()).toBe("Ctrl+Alt+K");
    h.held = "Ctrl+Alt+J";
    expect(heldCaptureChord()).toBe("Ctrl+Alt+J");
  });
});
