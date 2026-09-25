import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import type { BufferDocument } from "../../src/types/buffer";

// The hero embeds the app, so a keyboard visitor tabs into its editor, where
// Tab indents. Escape then Tab is CodeMirror's way out, and it only works when
// the Escape keydown reaches the editor rather than the app's command map.

const mocks = vi.hoisted(() => ({
  activeTabs: vi.fn<() => BufferDocument[]>(() => []),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../src/stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: mocks.activeTabs },
}));

vi.mock("../../src/services/tauri", () => ({
  previewGetLayout: mocks.previewGetLayout,
  previewSetLayout: mocks.previewSetLayout,
}));

import { getAllCommands, unregisterCommand } from "../../src/commands/registry";
import { installKeyboardHandler, rebuildKeyMap, uninstallKeyboardHandler } from "../../src/commands/keybindings";
import { registerPreviewKeymap } from "../../src/keymap/preview";
import { windowRegistry } from "../../src/stores/global/window-registry";
import { createWindowState } from "../../src/stores/window/createWindowState";
import { defaultSplit } from "../../src/lib/preview-layout";

const ESCAPE = { key: "Escape", code: "Escape", keyCode: 27 };
const TAB = { key: "Tab", code: "Tab", keyCode: 9 };

function buffer(id: string, name: string): BufferDocument {
  return {
    id,
    title: name,
    filename: name,
    status: "active",
    language: null,
    source_path: `/files/${name}`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

/** Dispatches a keydown the way a key press arrives, and reports whether anything consumed it. */
function press(target: HTMLElement, key: { key: string; code: string; keyCode: number }, shiftKey = false): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key: key.key, code: key.code, shiftKey, bubbles: true, cancelable: true });
  Object.defineProperty(event, "keyCode", { value: key.keyCode });
  Object.defineProperty(event, "which", { value: key.keyCode });
  target.dispatchEvent(event);
  return event;
}

describe("the embedded editor under the app's keyboard handler", () => {
  let view: EditorView;
  let windowId = 100;
  let release: (() => void) | null = null;

  beforeEach(() => {
    windowId += 1;
    release = windowRegistry.register(createWindowState({ windowId }));
    windowRegistry.focus(windowId);
    registerPreviewKeymap();
    rebuildKeyMap();
    installKeyboardHandler();
    view = new EditorView({
      state: EditorState.create({ doc: "one\ntwo", extensions: [keymap.of([indentWithTab])] }),
      parent: document.body,
    });
    view.focus();
  });

  afterEach(() => {
    uninstallKeyboardHandler();
    view.destroy();
    for (const command of [...getAllCommands()]) unregisterCommand(command.id);
    rebuildKeyMap();
    release?.();
    release = null;
    mocks.activeTabs.mockReturnValue([]);
  });

  it("indents on Tab while the visitor is typing", () => {
    const tab = press(view.contentDOM, TAB);
    expect(tab.defaultPrevented).toBe(true);
    expect(view.state.doc.line(1).text).not.toBe("one");
  });

  it("lets Escape reach the editor, so the next Tab or Shift+Tab moves focus instead of indenting", () => {
    for (const shiftKey of [false, true]) {
      const escape = press(view.contentDOM, ESCAPE);
      expect(escape.defaultPrevented).toBe(false);
      const tab = press(view.contentDOM, TAB, shiftKey);
      expect(tab.defaultPrevented, shiftKey ? "Shift+Tab" : "Tab").toBe(false);
      expect(view.state.doc.toString()).toBe("one\ntwo");
    }
  });

  it("still spends Escape on leaving a fullscreen preview, where the key has a job", () => {
    mocks.activeTabs.mockReturnValue([buffer("E1", "page.html")]);
    const win = windowRegistry.getActive()!;
    win.tabs.setActiveTabId("E1");
    win.layout.setLocal("E1", defaultSplit());
    win.layout.set("E1", "/files/page.html", { kind: "preview" });

    const escape = press(view.contentDOM, ESCAPE);
    expect(escape.defaultPrevented).toBe(true);
    expect(win.layout.get("E1", "html")).toEqual(defaultSplit());
  });
});
