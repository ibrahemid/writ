import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
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

  it("no longer parses the dead +double+ engine; the literal chord never matches", () => {
    let executed = false;
    registerCommand({
      id: "test.deaddouble",
      label: "Dead Double",
      keybinding: "CmdOrCtrl+double+S",
      scope: "app",
      execute: () => { executed = true; },
    });
    rebuildKeyMap();

    handleKeyDown(createKeyEvent({ key: "s", metaKey: true }));
    handleKeyDown(createKeyEvent({ key: "s", metaKey: true }));
    expect(executed).toBe(false);

    unregisterCommand("test.deaddouble");
    rebuildKeyMap();
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

describe("editor-focus gating", () => {
  let editor: HTMLDivElement;

  beforeEach(() => {
    for (const c of [...getAllCommands()]) unregisterCommand(c.id);
    editor = document.createElement("div");
    editor.className = "cm-editor";
    editor.tabIndex = -1;
    document.body.appendChild(editor);
  });

  afterEach(() => {
    for (const c of [...getAllCommands()]) unregisterCommand(c.id);
    if (editor.parentNode) editor.parentNode.removeChild(editor);
    document.body.focus();
    rebuildKeyMap();
  });

  function focusEditor() {
    editor.focus();
    // Precondition: a broken focus mock would leave activeElement on body and
    // silently turn the regression test green for the wrong reason.
    expect(editor.contains(document.activeElement)).toBe(true);
  }

  function keyEvent(overrides: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
    return {
      key: overrides.key,
      metaKey: overrides.metaKey ?? false,
      ctrlKey: overrides.ctrlKey ?? false,
      shiftKey: overrides.shiftKey ?? false,
      altKey: overrides.altKey ?? false,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    } as unknown as KeyboardEvent;
  }

  it("fires Cmd+S (sidebar.toggle) while the editor is focused — it is global", () => {
    // The editor holds focus almost all the time in a writing app, so a
    // focus-gated sidebar toggle is unreachable from the keyboard (and once the
    // sidebar opens, focus moves to its search input, also a gated text entry,
    // so it can never be closed). sidebar.toggle MUST be registered global; this
    // mirrors App.tsx and guards against re-gating it.
    let toggled = false;
    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle Sidebar",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      global: true,
      execute: () => { toggled = true; },
    });
    rebuildKeyMap();
    focusEditor();

    const event = keyEvent({ key: "s", metaKey: true });
    const handled = handleKeyDown(event);

    expect(handled).toBe(true);
    expect(toggled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalled();
  });

  it("still fires a deliberately-global app command (Cmd+T buffer.new) while typing", () => {
    let executed = false;
    registerCommand({
      id: "buffer.new",
      label: "New Tab",
      keybinding: "CmdOrCtrl+T",
      scope: "app",
      global: true,
      execute: () => { executed = true; },
    });
    rebuildKeyMap();
    focusEditor();

    const handled = handleKeyDown(keyEvent({ key: "t", metaKey: true }));
    expect(handled).toBe(true);
    expect(executed).toBe(true);
  });

  it("still fires a deliberately-global app command (Cmd+W buffer.close) while typing", () => {
    let executed = false;
    registerCommand({
      id: "buffer.close",
      label: "Close Tab",
      keybinding: "CmdOrCtrl+W",
      scope: "app",
      global: true,
      execute: () => { executed = true; },
    });
    rebuildKeyMap();
    focusEditor();

    const handled = handleKeyDown(keyEvent({ key: "w", metaKey: true }));
    expect(handled).toBe(true);
    expect(executed).toBe(true);
  });

  it("still fires an editor-scoped command (Cmd+F editor.find) while the editor is focused", () => {
    let executed = false;
    registerCommand({
      id: "editor.find",
      label: "Find",
      keybinding: "CmdOrCtrl+F",
      scope: "editor",
      execute: () => { executed = true; },
    });
    rebuildKeyMap();
    focusEditor();

    const handled = handleKeyDown(keyEvent({ key: "f", metaKey: true }));
    expect(handled).toBe(true);
    expect(executed).toBe(true);
  });

  it("fires Cmd+S (sidebar.toggle) when focus is outside any editor or input", () => {
    let toggled = false;
    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle Sidebar",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      execute: () => { toggled = true; },
    });
    rebuildKeyMap();
    document.body.focus();
    expect(editor.contains(document.activeElement)).toBe(false);

    const event = keyEvent({ key: "s", metaKey: true });
    const handled = handleKeyDown(event);

    expect(handled).toBe(true);
    expect(toggled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalled();
  });

  it("suppresses a non-global app command while focus is in a text input", () => {
    const input = document.createElement("input");
    document.body.appendChild(input);
    let toggled = false;
    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle Sidebar",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      execute: () => { toggled = true; },
    });
    rebuildKeyMap();
    input.focus();
    expect(document.activeElement).toBe(input);

    const handled = handleKeyDown(keyEvent({ key: "s", metaKey: true }));
    expect(handled).toBe(false);
    expect(toggled).toBe(false);

    input.parentNode?.removeChild(input);
  });

  it("suppresses tab.rename (Cmd+Shift+S) while the editor is focused", () => {
    let renamed = false;
    registerCommand({
      id: "tab.rename",
      label: "Rename Tab",
      keybinding: "F2",
      keybindingAliases: ["CmdOrCtrl+Shift+S"],
      scope: "app",
      execute: () => { renamed = true; },
    });
    rebuildKeyMap();
    focusEditor();

    const handled = handleKeyDown(keyEvent({ key: "s", metaKey: true, shiftKey: true }));
    expect(handled).toBe(false);
    expect(renamed).toBe(false);
  });

  it("keeps Shift+Shift palette global while the editor is focused", () => {
    let executed = false;
    registerCommand({
      id: "palette.open",
      label: "Command Palette",
      keybinding: "Shift+Shift",
      scope: "app",
      global: true,
      execute: () => { executed = true; },
    });
    rebuildKeyMap();
    focusEditor();

    handleKeyDown(keyEvent({ key: "Shift", shiftKey: true }));
    handleKeyDown(keyEvent({ key: "Shift", shiftKey: true }));
    expect(executed).toBe(true);
  });
});
