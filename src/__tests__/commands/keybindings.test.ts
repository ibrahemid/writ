import { describe, it, expect, beforeEach } from "vitest";
import { registerCommand, getAllCommands } from "../../commands/registry";
import {
  rebuildKeyMap,
  handleKeyDown,
  setKeybindingOverrides,
  effectiveBinding,
  pruneLegacyDefaultOverrides,
} from "../../commands/keybindings";

function createKeyEvent(overrides: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  return {
    key: overrides.key,
    metaKey: overrides.metaKey ?? false,
    ctrlKey: overrides.ctrlKey ?? false,
    shiftKey: overrides.shiftKey ?? false,
    altKey: overrides.altKey ?? false,
    preventDefault: () => {},
    stopPropagation: () => {},
  } as unknown as KeyboardEvent;
}

describe("keybindings", () => {
  beforeEach(() => {
    rebuildKeyMap();
  });

  it("normalizes Cmd+N correctly", () => {
    let executed = false;
    registerCommand({
      id: "test.cmd",
      label: "Test",
      keybinding: "CmdOrCtrl+N",
      scope: "app",
      execute: () => { executed = true; },
    });
    rebuildKeyMap();

    const event = createKeyEvent({ key: "n", metaKey: true });
    const handled = handleKeyDown(event);
    expect(handled).toBe(true);
    expect(executed).toBe(true);
  });

  it("detects Shift+Shift double tap", () => {
    let executed = false;
    registerCommand({
      id: "test.shiftshift",
      label: "Test Shift",
      keybinding: "Shift+Shift",
      scope: "app",
      execute: () => { executed = true; },
    });
    rebuildKeyMap();

    const shift1 = createKeyEvent({ key: "Shift", shiftKey: true });
    handleKeyDown(shift1);

    const shift2 = createKeyEvent({ key: "Shift", shiftKey: true });
    handleKeyDown(shift2);

    expect(executed).toBe(true);
  });

  it("does not trigger on unregistered key", () => {
    rebuildKeyMap();
    const event = createKeyEvent({ key: "x", metaKey: true });
    const handled = handleKeyDown(event);
    expect(handled).toBe(false);
  });

  it("routes aliases to the same command", () => {
    let executed = 0;
    registerCommand({
      id: "test.alias",
      label: "Test Alias",
      keybinding: "CmdOrCtrl+R",
      keybindingAliases: ["F2", "CmdOrCtrl+Shift+S"],
      scope: "app",
      execute: () => { executed += 1; },
    });
    rebuildKeyMap();

    handleKeyDown(createKeyEvent({ key: "r", metaKey: true }));
    handleKeyDown(createKeyEvent({ key: "F2" }));
    handleKeyDown(createKeyEvent({ key: "s", metaKey: true, shiftKey: true }));

    expect(executed).toBe(3);
  });

  it("ignores unset aliases", () => {
    registerCommand({
      id: "test.noalias",
      label: "Test No Alias",
      keybinding: "CmdOrCtrl+G",
      scope: "app",
      execute: () => {},
    });
    rebuildKeyMap();
    const handled = handleKeyDown(createKeyEvent({ key: "g", metaKey: true }));
    expect(handled).toBe(true);
  });

  it("does not assert getAllCommands count (registry persists across tests)", () => {
    expect(getAllCommands().length).toBeGreaterThan(0);
  });

  it("applies an override so handleKeyDown routes the new chord", () => {
    let executed = false;
    registerCommand({
      id: "test.override",
      label: "Test Override",
      keybinding: "CmdOrCtrl+H",
      scope: "app",
      execute: () => { executed = true; },
    });
    setKeybindingOverrides({ "test.override": "CmdOrCtrl+J" });
    rebuildKeyMap();

    const oldChord = handleKeyDown(createKeyEvent({ key: "h", metaKey: true }));
    const newChord = handleKeyDown(createKeyEvent({ key: "j", metaKey: true }));

    expect(oldChord).toBe(false);
    expect(newChord).toBe(true);
    expect(executed).toBe(true);

    setKeybindingOverrides({});
    rebuildKeyMap();
  });

  it("treats an empty override as 'no shortcut'", () => {
    setKeybindingOverrides({ "palette.open": "" });
    expect(effectiveBinding("palette.open", "Shift+Shift")).toBeUndefined();
    setKeybindingOverrides({});
  });
});

describe("pruneLegacyDefaultOverrides", () => {
  it("drops entries whose value matches the historical default for that id", () => {
    const pruned = pruneLegacyDefaultOverrides({
      "palette.open": "CmdOrCtrl+Shift+P",
      "buffer.new": "CmdOrCtrl+N",
      "user.choice": "CmdOrCtrl+Alt+X",
    });
    expect(pruned).toEqual({ "user.choice": "CmdOrCtrl+Alt+X" });
  });

  it("retains entries the user has customized away from the legacy default", () => {
    const pruned = pruneLegacyDefaultOverrides({
      "palette.open": "CmdOrCtrl+K",
    });
    expect(pruned).toEqual({ "palette.open": "CmdOrCtrl+K" });
  });

  it("returns an empty object when only legacy defaults are present", () => {
    const pruned = pruneLegacyDefaultOverrides({
      "palette.open": "CmdOrCtrl+Shift+P",
    });
    expect(pruned).toEqual({});
  });
});
