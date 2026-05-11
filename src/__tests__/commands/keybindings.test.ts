import { describe, it, expect, beforeEach } from "vitest";
import { registerCommand, getAllCommands } from "../../commands/registry";
import { rebuildKeyMap, handleKeyDown } from "../../commands/keybindings";

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
});
