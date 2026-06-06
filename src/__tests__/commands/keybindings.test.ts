import { describe, it, expect, beforeEach } from "vitest";
import { registerCommand, unregisterCommand, getAllCommands } from "../../commands/registry";
import {
  rebuildKeyMap,
  handleKeyDown,
  setKeybindingOverrides,
  effectiveBinding,
  findKeybindingConflicts,
  pruneLegacyDefaultOverrides,
} from "../../commands/keybindings";
import type { Command } from "../../types/commands";

function cmd(id: string, keybinding?: string, keybindingAliases?: string[]): Command {
  return { id, label: id, scope: "app", keybinding, keybindingAliases, execute: () => {} };
}

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

describe("editor.replace vs tab.rename chord", () => {
  it("resolves Cmd+R to editor.replace, not tab.rename", () => {
    // Neutralize the Cmd+R owner registered by the alias test above.
    unregisterCommand("test.alias");

    let replaced = 0;
    let renamed = 0;
    registerCommand({
      id: "editor.replace",
      label: "Replace",
      keybinding: "CmdOrCtrl+R",
      keybindingAliases: ["CmdOrCtrl+Alt+F"],
      scope: "editor",
      execute: () => { replaced += 1; },
    });
    registerCommand({
      id: "tab.rename",
      label: "Rename Tab",
      keybinding: "F2",
      keybindingAliases: ["CmdOrCtrl+Shift+S"],
      scope: "app",
      execute: () => { renamed += 1; },
    });
    rebuildKeyMap();

    expect(handleKeyDown(createKeyEvent({ key: "r", metaKey: true }))).toBe(true);
    expect(replaced).toBe(1);
    expect(renamed).toBe(0);

    // Rename still reachable via its own chords.
    handleKeyDown(createKeyEvent({ key: "F2" }));
    expect(renamed).toBe(1);

    unregisterCommand("editor.replace");
    unregisterCommand("tab.rename");
    rebuildKeyMap();
  });
});

describe("findKeybindingConflicts", () => {
  it("returns no conflicts for distinct chords", () => {
    const conflicts = findKeybindingConflicts([
      cmd("a", "CmdOrCtrl+R"),
      cmd("b", "CmdOrCtrl+T"),
    ]);
    expect(conflicts.size).toBe(0);
  });

  it("flags two commands sharing a chord", () => {
    const conflicts = findKeybindingConflicts([
      cmd("a", "CmdOrCtrl+R"),
      cmd("b", "CmdOrCtrl+R"),
    ]);
    expect(conflicts.get("CmdOrCtrl+R")).toEqual(["a", "b"]);
  });

  it("flags a primary chord colliding with another command's alias", () => {
    const conflicts = findKeybindingConflicts([
      cmd("a", "F2"),
      cmd("b", "CmdOrCtrl+R", ["F2"]),
    ]);
    expect(conflicts.get("F2")).toEqual(["a", "b"]);
  });

  it("does not flag a command whose own primary equals its alias", () => {
    const conflicts = findKeybindingConflicts([cmd("a", "F2", ["F2"])]);
    expect(conflicts.size).toBe(0);
  });

  it("treats the real editor.replace and tab.rename bindings as conflict-free", () => {
    const conflicts = findKeybindingConflicts([
      cmd("editor.replace", "CmdOrCtrl+R", ["CmdOrCtrl+Alt+F"]),
      cmd("tab.rename", "F2", ["CmdOrCtrl+Shift+S"]),
    ]);
    expect(conflicts.size).toBe(0);
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
