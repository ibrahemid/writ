import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { registerCommand, unregisterCommand, getAllCommands } from "../../commands/registry";
import { rebuildKeyMap, handleKeyDown } from "../../commands/keybindings";
import { registerEditorCommands } from "../../editor/editor-commands";
import { EDITOR_COMMAND_KEYS } from "../../editor/editor-command-keys";

// The chords Apple's own apps have claimed for something else. Each one is
// checked where it is dispatched, not where it is declared: a table entry
// nothing routes would pass a data assertion and still leave the key live.

function keyEvent(overrides: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  return {
    key: overrides.key,
    metaKey: overrides.metaKey ?? false,
    ctrlKey: overrides.ctrlKey ?? false,
    shiftKey: overrides.shiftKey ?? false,
    altKey: overrides.altKey ?? false,
    isComposing: false,
    keyCode: 0,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  } as unknown as KeyboardEvent;
}

function clearRegistry() {
  for (const command of [...getAllCommands()]) unregisterCommand(command.id);
}

describe("Cmd+E and Cmd+Shift+K", () => {
  let host: HTMLDivElement;
  let view: EditorView;
  let dispose: () => void;

  beforeEach(() => {
    clearRegistry();
    host = document.createElement("div");
    document.body.appendChild(host);
    view = new EditorView({
      state: EditorState.create({ doc: "first\nsecond\nthird" }),
      parent: host,
    });
    view.dom.tabIndex = -1;
    view.focus();
    view.dom.focus();
    dispose = registerEditorCommands(() => view);
  });

  afterEach(() => {
    dispose();
    view.destroy();
    host.remove();
    clearRegistry();
    rebuildKeyMap();
  });

  it("leaves the line alone on Cmd+E", () => {
    const before = view.state.doc.toString();
    expect(handleKeyDown(keyEvent({ key: "e", metaKey: true }))).toBe(false);
    expect(view.state.doc.toString()).toBe(before);
  });

  it("deletes the line on Cmd+Shift+K", () => {
    expect(handleKeyDown(keyEvent({ key: "k", metaKey: true, shiftKey: true }))).toBe(true);
    expect(view.state.doc.toString()).toBe("second\nthird");
  });

  it("holds Cmd+Shift+K as the one chord the table declares for it", () => {
    const entry = EDITOR_COMMAND_KEYS.find((key) => key.id === "editor.deleteLine");
    expect(entry?.keybinding).toBe("CmdOrCtrl+Shift+K");
    expect(entry?.aliases ?? []).toEqual([]);
  });
});

describe("Cmd+Option+S", () => {
  afterEach(() => {
    clearRegistry();
    rebuildKeyMap();
  });

  it("toggles the sidebar alongside Cmd+\\", () => {
    let toggled = 0;
    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle sidebar",
      keybinding: "CmdOrCtrl+\\",
      keybindingAliases: ["CmdOrCtrl+Alt+S"],
      scope: "app",
      global: true,
      execute: () => {
        toggled += 1;
      },
    });
    rebuildKeyMap();

    expect(handleKeyDown(keyEvent({ key: "\\", metaKey: true }))).toBe(true);
    expect(handleKeyDown(keyEvent({ key: "s", metaKey: true, altKey: true }))).toBe(true);
    expect(toggled).toBe(2);
  });
});
