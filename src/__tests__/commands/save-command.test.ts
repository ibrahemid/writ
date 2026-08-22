import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
  searchBuffers: vi.fn().mockResolvedValue({ hits: [], total: 0 }),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { registerCommand, unregisterCommand, getAllCommands } from "../../commands/registry";
import { rebuildKeyMap, handleKeyDown } from "../../commands/keybindings";
import { createEditorStore, type EditorStore } from "../../stores/window/editor-store";
import { createSidebarStore } from "../../stores/window/sidebar-store";
import { saveStatusStore } from "../../stores/global/save-status";
import { debouncedSave } from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

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

describe("Cmd+S saves the active buffer", () => {
  let view: EditorView;
  let container: HTMLDivElement;
  let store: EditorStore;

  beforeEach(() => {
    vi.clearAllMocks();
    mockedSave.mockResolvedValue(undefined);
    for (const c of [...getAllCommands()]) unregisterCommand(c.id);

    container = document.createElement("div");
    document.body.appendChild(container);
    view = new EditorView({
      state: EditorState.create({ doc: "hello disk" }),
      parent: container,
    });
    // The view's own element carries `.cm-editor`; making it focusable lets the
    // scope gate see real editor focus instead of a stand-in element.
    view.dom.tabIndex = -1;

    store = createEditorStore();
    store.registerView(view);
    store.setCurrentBufferId("buf-1");

    registerCommand({
      id: "buffer.save",
      label: "Save",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      global: true,
      execute: () => void store.saveActiveBuffer(),
    });
    rebuildKeyMap();
  });

  afterEach(() => {
    for (const c of [...getAllCommands()]) unregisterCommand(c.id);
    rebuildKeyMap();
    view.destroy();
    container.remove();
    document.body.focus();
  });

  function focusEditor() {
    view.dom.focus();
    expect(view.dom.contains(document.activeElement)).toBe(true);
  }

  it("writes the live document and shows the saved status, with no edit pending", async () => {
    focusEditor();

    const event = keyEvent({ key: "s", metaKey: true });
    expect(handleKeyDown(event)).toBe(true);
    expect(event.preventDefault).toHaveBeenCalled();

    await vi.waitFor(() => expect(mockedSave).toHaveBeenCalledWith("buf-1", "hello disk"));
    expect(saveStatusStore.status()).toBe("saved");
  });

  it("writes once when an autosave was already scheduled", async () => {
    vi.useFakeTimers();
    try {
      debouncedSave("buf-1", () => view.state.doc.toString(), 300);
      focusEditor();

      expect(handleKeyDown(keyEvent({ key: "s", metaKey: true }))).toBe(true);
      await vi.waitFor(() => expect(mockedSave).toHaveBeenCalledTimes(1));

      await vi.advanceTimersByTimeAsync(1000);
      expect(mockedSave).toHaveBeenCalledTimes(1);
      expect(mockedSave).toHaveBeenCalledWith("buf-1", "hello disk");
    } finally {
      vi.useRealTimers();
    }
  });

  it("surfaces a failed write through the save status", async () => {
    mockedSave.mockRejectedValueOnce(new Error("disk full"));
    focusEditor();

    expect(handleKeyDown(keyEvent({ key: "s", metaKey: true }))).toBe(true);

    await vi.waitFor(() => expect(saveStatusStore.status()).toBe("failed"));
  });

  it("does not write when no buffer is loaded into the view", async () => {
    store.setCurrentBufferId(null);
    focusEditor();

    expect(handleKeyDown(keyEvent({ key: "s", metaKey: true }))).toBe(true);

    await Promise.resolve();
    expect(mockedSave).not.toHaveBeenCalled();
  });

  it("does not write back a binary buffer, which opens read-only", async () => {
    store.setLargeFileMode({ kind: "Binary" });
    focusEditor();

    expect(handleKeyDown(keyEvent({ key: "s", metaKey: true }))).toBe(true);

    await Promise.resolve();
    expect(mockedSave).not.toHaveBeenCalled();
  });
});

describe("Cmd+\\ toggles the sidebar", () => {
  afterEach(() => {
    for (const c of [...getAllCommands()]) unregisterCommand(c.id);
    rebuildKeyMap();
  });

  it("flips the sidebar on the new chord", () => {
    const sidebar = createSidebarStore();
    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle Sidebar",
      keybinding: "CmdOrCtrl+\\",
      scope: "app",
      global: true,
      execute: () => sidebar.toggle(),
    });
    rebuildKeyMap();

    expect(sidebar.isOpen()).toBe(false);

    const event = keyEvent({ key: "\\", metaKey: true });
    expect(handleKeyDown(event)).toBe(true);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(sidebar.isOpen()).toBe(true);

    expect(handleKeyDown(keyEvent({ key: "\\", metaKey: true }))).toBe(true);
    expect(sidebar.isOpen()).toBe(false);
  });
});
