import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  registerCommand,
  unregisterCommand,
  getAllCommands,
} from "../../commands/registry";
import { rebuildKeyMap, handleKeyDown } from "../../commands/keybindings";
import { registerEditorCommands } from "../../editor/editor-commands";

function keyEvent(
  overrides: Partial<KeyboardEvent> & { key: string },
): KeyboardEvent {
  return {
    key: overrides.key,
    metaKey: overrides.metaKey ?? false,
    ctrlKey: overrides.ctrlKey ?? false,
    shiftKey: overrides.shiftKey ?? false,
    altKey: overrides.altKey ?? false,
    isComposing: overrides.isComposing ?? false,
    keyCode: overrides.keyCode ?? 0,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  } as unknown as KeyboardEvent;
}

function clearRegistry() {
  for (const c of [...getAllCommands()]) unregisterCommand(c.id);
}

describe("editor-command scope gating", () => {
  let editor: HTMLDivElement;

  beforeEach(() => {
    clearRegistry();
    editor = document.createElement("div");
    editor.className = "cm-editor";
    editor.tabIndex = -1;
    document.body.appendChild(editor);
  });

  afterEach(() => {
    clearRegistry();
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
    document.body.focus();
    rebuildKeyMap();
  });

  function registerEditorDup(execute: () => boolean | void) {
    registerCommand({
      id: "editor.test",
      label: "Editor Test",
      keybinding: "CmdOrCtrl+D",
      scope: "editor",
      execute,
    });
    rebuildKeyMap();
  }

  it("fires an editor command when focus is inside .cm-editor", () => {
    let fired = false;
    registerEditorDup(() => {
      fired = true;
    });
    editor.focus();
    expect(editor.contains(document.activeElement)).toBe(true);

    const e = keyEvent({ key: "d", metaKey: true });
    expect(handleKeyDown(e)).toBe(true);
    expect(fired).toBe(true);
    expect(e.preventDefault).toHaveBeenCalled();
  });

  it("does not fire an editor command when focus is in the find input", () => {
    const input = document.createElement("input");
    input.className = "find-input";
    document.body.appendChild(input);
    let fired = false;
    registerEditorDup(() => {
      fired = true;
    });
    input.focus();
    expect(document.activeElement).toBe(input);

    const e = keyEvent({ key: "d", metaKey: true });
    expect(handleKeyDown(e)).toBe(false);
    expect(fired).toBe(false);
    expect(e.preventDefault).not.toHaveBeenCalled();
  });

  it("does not fire an editor command when focus is in the tab-rename input", () => {
    const input = document.createElement("input");
    input.className = "tab-rename-input";
    document.body.appendChild(input);
    let fired = false;
    registerEditorDup(() => {
      fired = true;
    });
    input.focus();

    expect(handleKeyDown(keyEvent({ key: "d", metaKey: true }))).toBe(false);
    expect(fired).toBe(false);
  });

  it("does not fire an editor command when focus is in the sidebar search", () => {
    const input = document.createElement("input");
    input.className = "sidebar-search";
    document.body.appendChild(input);
    let fired = false;
    registerEditorDup(() => {
      fired = true;
    });
    input.focus();

    expect(handleKeyDown(keyEvent({ key: "d", metaKey: true }))).toBe(false);
    expect(fired).toBe(false);
  });

  it("leaves defaultPrevented false when the command returns false (fall-through)", () => {
    registerEditorDup(() => false);
    editor.focus();

    const e = keyEvent({ key: "d", metaKey: true });
    expect(handleKeyDown(e)).toBe(false);
    expect(e.preventDefault).not.toHaveBeenCalled();
    expect(e.stopPropagation).not.toHaveBeenCalled();
  });

  it("matches nothing while an IME is composing", () => {
    let fired = false;
    registerEditorDup(() => {
      fired = true;
    });
    editor.focus();

    expect(handleKeyDown(keyEvent({ key: "d", metaKey: true, isComposing: true }))).toBe(false);
    expect(handleKeyDown(keyEvent({ key: "Process", keyCode: 229, metaKey: true }))).toBe(false);
    expect(fired).toBe(false);
  });
});

describe("editor table resolves immediately after registration", () => {
  afterEach(() => {
    clearRegistry();
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
    document.body.focus();
    rebuildKeyMap();
  });

  it("a table chord dispatches through handleKeyDown with no other effect having run", () => {
    clearRegistry();
    const state = EditorState.create({
      doc: "abc",
      selection: EditorSelection.cursor(1),
    });
    const view = new EditorView({ state, parent: document.body });
    // registerEditorCommands rebuilds the chord map itself — nothing else runs.
    const dispose = registerEditorCommands(() => view);
    view.focus();
    expect(view.dom.contains(document.activeElement)).toBe(true);

    const e = keyEvent({ key: "d", metaKey: true });
    expect(handleKeyDown(e)).toBe(true);
    expect(view.state.doc.toString()).toBe("abc\nabc");

    dispose();
    view.destroy();
  });
});
