import { describe, it, expect, afterEach, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  showContextMenu: vi.fn(),
  readClipboardText: vi.fn().mockResolvedValue(""),
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
}));

vi.mock("../../services/clipboard", () => ({
  readClipboardText: mocks.readClipboardText,
  writeClipboardText: mocks.writeClipboardText,
}));

import { installNativeContextMenuSuppressor } from "../../lib/native-context-menu";

let dispose: (() => void) | null = null;

afterEach(() => {
  dispose?.();
  dispose = null;
  document.body.innerHTML = "";
  mocks.showContextMenu.mockReset();
});

function rightClick(target: EventTarget): MouseEvent {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    clientX: 20,
    clientY: 30,
  });
  target.dispatchEvent(event);
  return event;
}

describe("native context menu suppression", () => {
  it("suppresses the engine menu on plain chrome and shows nothing", () => {
    dispose = installNativeContextMenuSuppressor();
    const div = document.createElement("div");
    document.body.append(div);

    const event = rightClick(div);
    expect(event.defaultPrevented).toBe(true);
    expect(mocks.showContextMenu).not.toHaveBeenCalled();
  });

  it("never stops propagation, so delegated menus still receive the event", () => {
    // Solid delegates `contextmenu` at the document: the tab bar and the three
    // sidebar menus all arrive through that one listener. Calling
    // stopPropagation here would kill every one of them.
    dispose = installNativeContextMenuSuppressor();
    const div = document.createElement("div");
    document.body.append(div);

    const seen = vi.fn();
    document.addEventListener("contextmenu", seen);
    try {
      rightClick(div);
      expect(seen).toHaveBeenCalledTimes(1);
    } finally {
      document.removeEventListener("contextmenu", seen);
    }
  });

  it("leaves a surface that already opened its own menu alone", () => {
    dispose = installNativeContextMenuSuppressor();
    const owner = document.createElement("div");
    document.body.append(owner);
    // What the editor extension and the tab bar do.
    owner.addEventListener("contextmenu", (e) => e.preventDefault());

    rightClick(owner);
    expect(mocks.showContextMenu).not.toHaveBeenCalled();
  });

  it("gives a text input its own menu", () => {
    dispose = installNativeContextMenuSuppressor();
    const input = document.createElement("input");
    input.type = "text";
    input.value = "hello world";
    document.body.append(input);
    input.setSelectionRange(0, 5);

    const event = rightClick(input);
    expect(event.defaultPrevented).toBe(true);
    expect(mocks.showContextMenu).toHaveBeenCalledTimes(1);

    const [x, y, items] = mocks.showContextMenu.mock.calls[0];
    expect([x, y]).toEqual([20, 30]);
    expect(items.map((i: { label: string }) => i.label)).toEqual([
      "Cut",
      "Copy",
      "Paste",
      "Select all",
    ]);
  });

  it("omits cut and copy in a field with no selection", () => {
    dispose = installNativeContextMenuSuppressor();
    const input = document.createElement("input");
    input.type = "text";
    input.value = "hello";
    document.body.append(input);
    input.setSelectionRange(2, 2);

    rightClick(input);
    const items = mocks.showContextMenu.mock.calls[0][2];
    expect(items.map((i: { label: string }) => i.label)).toEqual(["Paste", "Select all"]);
  });

  it("covers textareas too", () => {
    dispose = installNativeContextMenuSuppressor();
    const area = document.createElement("textarea");
    area.value = "text";
    document.body.append(area);

    rightClick(area);
    expect(mocks.showContextMenu).toHaveBeenCalledTimes(1);
  });

  it("ignores inputs with no text to act on", () => {
    dispose = installNativeContextMenuSuppressor();
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    document.body.append(checkbox);

    rightClick(checkbox);
    expect(mocks.showContextMenu).not.toHaveBeenCalled();
  });

  it("disables the editing verbs on a read-only field", () => {
    dispose = installNativeContextMenuSuppressor();
    const input = document.createElement("input");
    input.type = "text";
    input.value = "locked";
    input.readOnly = true;
    document.body.append(input);
    input.setSelectionRange(0, 6);

    rightClick(input);
    const items = mocks.showContextMenu.mock.calls[0][2];
    const byLabel = Object.fromEntries(items.map((i: { label: string }) => [i.label, i]));
    expect(byLabel["Cut"].disabled).toBe(true);
    expect(byLabel["Paste"].disabled).toBe(true);
    expect(byLabel["Copy"].disabled).toBeFalsy();
  });

  it("stops suppressing once disposed", () => {
    const stop = installNativeContextMenuSuppressor();
    stop();
    const div = document.createElement("div");
    document.body.append(div);

    expect(rightClick(div).defaultPrevented).toBe(false);
  });
});
