import { describe, it, expect } from "vitest";
import { formatKeybinding } from "../../lib/keybinding-format";

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

  it("preserves function key bindings unchanged", () => {
    expect(formatKeybinding("F2", { isMac: true })).toBe("F2");
    expect(formatKeybinding("F2", { isMac: false })).toBe("F2");
  });

  it("preserves single-key bindings unchanged", () => {
    expect(formatKeybinding("Escape", { isMac: true })).toBe("Escape");
  });
});
