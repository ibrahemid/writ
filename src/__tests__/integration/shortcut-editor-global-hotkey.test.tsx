import { describe, it, expect, vi, beforeEach, afterEach, beforeAll } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
  saveConfig: vi.fn().mockResolvedValue(undefined),
  setGlobalHotkey: vi.fn(),
  globalHotkeyStatus: vi.fn(),
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ keybindings: {}, hotkey: { toggle: "CmdOrCtrl+Shift+Space" } }),
    save: h.saveConfig,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../services/tauri", () => ({
  setGlobalHotkey: h.setGlobalHotkey,
  globalHotkeyStatus: h.globalHotkeyStatus,
}));

vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));

import ShortcutEditor, {
  openShortcutEditor,
  closeShortcutEditor,
} from "../../components/ShortcutEditor/ShortcutEditor";
import { hotkeyStore } from "../../stores/global/hotkey";
import { keybindingSegments } from "../../lib/keybinding-format";
import { getAllCommands, registerCommand, unregisterCommand } from "../../commands/registry";

function row(container: HTMLElement): HTMLElement {
  const found = container.querySelector<HTMLElement>('[data-shortcut="global-toggle"]');
  expect(found, "the shortcut editor carries a row for the window's own chord").not.toBeNull();
  return found!;
}

/** One ordinary row to edit alongside the window's chord. */
function registerRenameCommand(): void {
  registerCommand({
    id: "note.rename",
    label: "Rename note",
    keybinding: "F2",
    scope: "app",
    execute: () => {},
  });
}

/** Lets the save handler's awaits, and the render they trigger, run out. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/** The chord an ordinary row is showing, as the row's keycaps spell it. */
function ordinaryRowChord(container: HTMLElement): string {
  const found = container.querySelector<HTMLElement>(".shortcut-row:not([data-shortcut])");
  expect(found, "the shortcut editor carries a row for the ordinary command").not.toBeNull();
  return [...found!.querySelectorAll(".kbd-key")].map((key) => key.textContent).join("");
}

function chordAsShown(binding: string): string {
  return keybindingSegments(binding).join("");
}

describe("the chord that shows and hides the window", () => {
  beforeEach(() => {
    for (const command of [...getAllCommands()]) unregisterCommand(command.id);
    h.setGlobalHotkey.mockReset();
    h.globalHotkeyStatus.mockReset();
  });

  afterEach(() => {
    closeShortcutEditor();
    cleanup();
  });

  it("shows the chord it holds, and no warning, while the OS gives it up", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    expect(row(container).textContent).toContain("Show and hide Writ");
    expect(row(container).querySelector('[data-state="taken"]')).toBeNull();
  });

  it("says the shortcut is taken, and offers a rebind, when another app holds it", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: false,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    const taken = row(container).querySelector('[data-state="taken"]');
    expect(taken?.textContent).toContain("Another app is using this shortcut");
    expect(
      row(container).querySelector('[data-action="record-global-shortcut"]'),
      "a taken chord is never a dead row: it offers the recorder",
    ).not.toBeNull();
  });

  it("records a new chord and asks the OS for it", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: false,
    });
    h.setGlobalHotkey.mockResolvedValue({ chord: "CmdOrCtrl+Alt+Space", registered: true });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
    fireEvent.keyDown(document, { key: "Space", metaKey: true, altKey: true });

    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await Promise.resolve();
    await Promise.resolve();

    expect(h.setGlobalHotkey).toHaveBeenCalledWith("CmdOrCtrl+Alt+Space");
    expect(hotkeyStore.isTaken()).toBe(false);
    expect(hotkeyStore.chord()).toBe("CmdOrCtrl+Alt+Space");
  });

  it("saves the chord the OS took, not the one that was asked for", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
    });
    h.setGlobalHotkey.mockResolvedValue({
      chord: "CmdOrCtrl+Alt+Space",
      registered: true,
    });
    h.saveConfig.mockClear();
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
    fireEvent.keyDown(document, { key: " ", metaKey: true, altKey: true });

    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await settle();

    expect(h.setGlobalHotkey).toHaveBeenCalledWith("CmdOrCtrl+Alt+Space");
    expect(h.saveConfig).toHaveBeenCalledWith(
      expect.objectContaining({ hotkey: { toggle: "CmdOrCtrl+Alt+Space" } }),
    );
  });

  // `set_global_hotkey` parses before it registers, so a chord the recorder can
  // capture but `writ_core::hotkey` cannot read comes back as a rejected call.
  // `hotkey/mod.rs` holds the Rust half: both of these chords are `Err` there.
  for (const [key, chord] of [
    ["Backspace", "CmdOrCtrl+Shift+Backspace"],
    ["F13", "CmdOrCtrl+F13"],
  ] as const) {
    it(`keeps the other rows, and says why, when ${chord} cannot be registered`, async () => {
      h.globalHotkeyStatus.mockResolvedValue({
        chord: "CmdOrCtrl+Shift+Space",
        registered: true,
      });
      h.setGlobalHotkey.mockRejectedValue(new Error("hotkey chord parse failed: unknown token"));
      h.saveConfig.mockClear();
      await hotkeyStore.load();
      registerRenameCommand();

      const { container } = render(() => <ShortcutEditor />);
      openShortcutEditor();

      // One ordinary row is edited first: that write is what a refusal used to
      // take down with it.
      fireEvent.click(container.querySelector('[data-action="record-shortcut"]')!);
      fireEvent.keyDown(document, { key: "r", metaKey: true, altKey: true });

      fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
      fireEvent.keyDown(document, {
        key,
        metaKey: true,
        shiftKey: chord.includes("Shift"),
      });

      fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
      await settle();

      expect(h.setGlobalHotkey).toHaveBeenCalledWith(chord);
      expect(row(container).querySelector('[data-state="unusable"]')?.textContent).toContain(
        "Writ can't use this shortcut",
      );
      expect(h.saveConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          keybindings: { "note.rename": "CmdOrCtrl+Alt+R" },
          hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
        }),
      );
    });
  }

  it("writes the keybindings, and shows the chord as taken, when the OS refuses it", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
    });
    h.setGlobalHotkey.mockResolvedValue({
      chord: "CmdOrCtrl+Alt+Space",
      registered: false,
    });
    h.saveConfig.mockClear();
    await hotkeyStore.load();
    registerRenameCommand();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    fireEvent.click(container.querySelector('[data-action="record-shortcut"]')!);
    fireEvent.keyDown(document, { key: "r", metaKey: true, altKey: true });

    fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
    fireEvent.keyDown(document, { key: " ", metaKey: true, altKey: true });

    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await settle();

    expect(h.saveConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        keybindings: { "note.rename": "CmdOrCtrl+Alt+R" },
        hotkey: { toggle: "CmdOrCtrl+Alt+Space" },
      }),
    );
    expect(hotkeyStore.isTaken()).toBe(true);
    expect(
      row(container).querySelector('[data-state="unusable"]'),
      "a chord the OS gave to another app is taken, not unusable",
    ).toBeNull();
    expect(
      row(container).textContent,
      "the row shows the chord that was asked for, under the warning that it is taken",
    ).toContain(chordAsShown("CmdOrCtrl+Alt+Space"));
  });

  // `hotkey:status` is emitted by the Rust side, not by the modal, so the chord
  // can move while the editor is up. It used to re-seed every row from the
  // config, which took recorded but unsaved chords down with it.
  it("keeps the recorded rows when the chord moves while the editor is open", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
    });
    h.setGlobalHotkey.mockResolvedValue({ chord: "CmdOrCtrl+Alt+Space", registered: true });
    await hotkeyStore.load();
    registerRenameCommand();

    const { container } = render(() => <ShortcutEditor />);
    openShortcutEditor();

    fireEvent.click(container.querySelector('[data-action="record-shortcut"]')!);
    fireEvent.keyDown(document, { key: "r", metaKey: true, altKey: true });
    expect(ordinaryRowChord(container)).toBe(chordAsShown("CmdOrCtrl+Alt+R"));

    // Nothing in the modal asked for this: it is the answer arriving on its own.
    await hotkeyStore.rebind("CmdOrCtrl+Alt+Space");
    await settle();

    expect(
      ordinaryRowChord(container),
      "a status from outside the modal must not reach the other rows",
    ).toBe(chordAsShown("CmdOrCtrl+Alt+R"));
    expect(
      row(container).textContent,
      "the row the status is about takes the chord the OS now holds",
    ).toContain(chordAsShown("CmdOrCtrl+Alt+Space"));
  });
});
