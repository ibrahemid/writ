import { describe, it, expect } from "vitest";
import {
  formatKeybinding,
  keybindingSegments,
  spokenKeybinding,
} from "../../lib/keybinding-format";

describe("formatKeybinding", () => {
  it("formats CmdOrCtrl+K to ⌘K on darwin", () => {
    expect(formatKeybinding("CmdOrCtrl+K", { isMac: true })).toBe("⌘K");
  });

  it("formats CmdOrCtrl+K to Ctrl+K on non-darwin", () => {
    expect(formatKeybinding("CmdOrCtrl+K", { isMac: false })).toBe("Ctrl+K");
  });

  it("formats Shift+Shift to ⇧ ⇧ on darwin", () => {
    expect(formatKeybinding("Shift+Shift", { isMac: true })).toBe("⇧ ⇧");
  });

  it("formats Shift+Shift to Shift Shift on non-darwin", () => {
    expect(formatKeybinding("Shift+Shift", { isMac: false })).toBe("Shift Shift");
  });

  it("returns empty string for undefined binding", () => {
    expect(formatKeybinding(undefined)).toBe("");
  });

  it("returns empty string for empty binding", () => {
    expect(formatKeybinding("")).toBe("");
  });

  it("formats CmdOrCtrl+Shift+T to ⌘⇧T on darwin", () => {
    expect(formatKeybinding("CmdOrCtrl+Shift+T", { isMac: true })).toBe("⌘⇧T");
  });

  it("formats CmdOrCtrl+Shift+T to Ctrl+Shift+T on non-darwin", () => {
    expect(formatKeybinding("CmdOrCtrl+Shift+T", { isMac: false })).toBe("Ctrl+Shift+T");
  });

  it("formats CmdOrCtrl+\\ to ⌘\\ on darwin and Ctrl+\\ elsewhere", () => {
    expect(formatKeybinding("CmdOrCtrl+\\", { isMac: true })).toBe("⌘\\");
    expect(formatKeybinding("CmdOrCtrl+\\", { isMac: false })).toBe("Ctrl+\\");
  });

  it("preserves function key bindings unchanged", () => {
    expect(formatKeybinding("F2", { isMac: true })).toBe("F2");
    expect(formatKeybinding("F2", { isMac: false })).toBe("F2");
  });

  it("shortens Escape to the word printed on the key", () => {
    expect(formatKeybinding("Escape", { isMac: true })).toBe("Esc");
    expect(formatKeybinding("Escape", { isMac: false })).toBe("Esc");
  });

  it("draws arrows as arrows rather than their DOM names", () => {
    expect(formatKeybinding("Shift+Alt+ArrowUp", { isMac: true })).toBe("⇧⌥↑");
    expect(formatKeybinding("Shift+Alt+ArrowDown", { isMac: false })).toBe("Shift+Alt+↓");
  });

  it("draws Enter as the return glyph on macOS", () => {
    expect(formatKeybinding("CmdOrCtrl+Enter", { isMac: true })).toBe("⌘↩");
    expect(formatKeybinding("CmdOrCtrl+Enter", { isMac: false })).toBe("Ctrl+Enter");
  });

  it("keeps the plus key of a chord that ends in it", () => {
    expect(formatKeybinding("CmdOrCtrl+Shift++", { isMac: true })).toBe("⌘⇧+");
    expect(formatKeybinding("CmdOrCtrl+Shift++", { isMac: false })).toBe("Ctrl+Shift++");
  });

  it("draws a macOS Control chord with the Control glyph", () => {
    expect(formatKeybinding("Ctrl+N", { isMac: true })).toBe("⌃N");
  });
});

describe("keybindingSegments", () => {
  it("returns two segments for Shift+Shift on darwin", () => {
    expect(keybindingSegments("Shift+Shift", { isMac: true })).toEqual(["⇧", "⇧"]);
  });

  it("returns two segments for Shift+Shift on non-darwin", () => {
    expect(keybindingSegments("Shift+Shift", { isMac: false })).toEqual(["Shift", "Shift"]);
  });

  it("splits CmdOrCtrl+K into modifier and key on darwin", () => {
    expect(keybindingSegments("CmdOrCtrl+K", { isMac: true })).toEqual(["⌘", "K"]);
  });

  it("splits CmdOrCtrl+Shift+T into three segments on darwin", () => {
    expect(keybindingSegments("CmdOrCtrl+Shift+T", { isMac: true })).toEqual(["⌘", "⇧", "T"]);
  });

  it("splits CmdOrCtrl+\\ into modifier and key on darwin", () => {
    expect(keybindingSegments("CmdOrCtrl+\\", { isMac: true })).toEqual(["⌘", "\\"]);
  });

  it("returns single segment for F2", () => {
    expect(keybindingSegments("F2", { isMac: true })).toEqual(["F2"]);
  });

  it("returns empty array for undefined", () => {
    expect(keybindingSegments(undefined)).toEqual([]);
  });
});

describe("spokenKeybinding", () => {
  it("says the modifiers by name for the platform", () => {
    expect(spokenKeybinding("CmdOrCtrl+Shift+P", { isMac: true })).toBe("Command Shift P");
    expect(spokenKeybinding("CmdOrCtrl+Shift+P", { isMac: false })).toBe("Control Shift P");
    expect(spokenKeybinding("CmdOrCtrl+Alt+F", { isMac: true })).toBe("Command Option F");
    expect(spokenKeybinding("Ctrl+N", { isMac: true })).toBe("Control N");
  });

  it("names arrows and punctuation", () => {
    expect(spokenKeybinding("Shift+Alt+ArrowUp", { isMac: false })).toBe("Shift Alt Up Arrow");
    expect(spokenKeybinding("CmdOrCtrl+\\", { isMac: true })).toBe("Command Backslash");
    expect(spokenKeybinding("CmdOrCtrl+Shift++", { isMac: true })).toBe("Command Shift Plus");
  });

  it("says nothing for no chord", () => {
    expect(spokenKeybinding(undefined)).toBe("");
  });
});
