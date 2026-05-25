import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { registerCommand } from "../../commands/registry";
import {
  installKeyboardHandler,
  uninstallKeyboardHandler,
  rebuildKeyMap,
} from "../../commands/keybindings";

function fireKeydown() {
  const event = new KeyboardEvent("keydown", {
    key: "j",
    metaKey: true,
    bubbles: true,
    cancelable: true,
  });
  document.dispatchEvent(event);
}

describe("keyboard handler install/uninstall lifecycle", () => {
  let executed: number;

  beforeEach(() => {
    executed = 0;
    registerCommand({
      id: "test.keyboard-lifecycle",
      label: "Test",
      keybinding: "CmdOrCtrl+J",
      scope: "app",
      execute: () => {
        executed += 1;
      },
    });
    rebuildKeyMap();
  });

  afterEach(() => {
    uninstallKeyboardHandler();
  });

  it("removes the keydown listener with matching capture option when uninstalled", () => {
    const addSpy = vi.spyOn(document, "addEventListener");
    const removeSpy = vi.spyOn(document, "removeEventListener");

    installKeyboardHandler();
    expect(addSpy).toHaveBeenCalledWith("keydown", expect.any(Function), {
      capture: true,
    });

    const addedHandler = addSpy.mock.calls.find(
      (call) => call[0] === "keydown",
    )?.[1];
    expect(addedHandler).toBeDefined();

    uninstallKeyboardHandler();
    expect(removeSpy).toHaveBeenCalledWith("keydown", addedHandler, {
      capture: true,
    });

    addSpy.mockRestore();
    removeSpy.mockRestore();
  });

  it("dispatched keydown stops triggering commands after uninstall", () => {
    installKeyboardHandler();
    fireKeydown();
    expect(executed).toBe(1);

    uninstallKeyboardHandler();
    fireKeydown();
    expect(executed).toBe(1);
  });

  it("install can be reapplied after uninstall", () => {
    installKeyboardHandler();
    uninstallKeyboardHandler();
    installKeyboardHandler();
    fireKeydown();
    expect(executed).toBe(1);
  });
});
