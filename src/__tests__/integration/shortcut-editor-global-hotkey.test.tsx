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
  toggle: "CmdOrCtrl+Shift+Space",
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ keybindings: {}, hotkey: { toggle: h.toggle } }),
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
  discardShortcutDrafts,
  hasUnsavedShortcuts,
  windowChordRule,
  WAYLAND_LINE,
} from "../../components/ShortcutEditor/ShortcutEditor";
import { detectPlatform } from "../../lib/platform";
import { hotkeyStore } from "../../stores/global/hotkey";
import { formatKeybinding, keybindingSegments } from "../../lib/keybinding-format";
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

/** The answer to a rebind the OS gave. */
function given(chord: string) {
  return { status: { chord, registered: true, unsupported: false }, refused: false };
}

function chordAsShown(binding: string): string {
  return keybindingSegments(binding).join("");
}

describe("the chord that shows and hides the window", () => {
  beforeEach(() => {
    for (const command of [...getAllCommands()]) unregisterCommand(command.id);
    h.setGlobalHotkey.mockReset();
    h.globalHotkeyStatus.mockReset();
    h.toggle = "CmdOrCtrl+Shift+Space";
  });

  afterEach(() => {
    discardShortcutDrafts();
    cleanup();
  });

  it("shows the chord it holds, and no warning, while the OS gives it up", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
      unsupported: false,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);

    expect(row(container).textContent).toContain("Show and hide Writ");
    expect(row(container).querySelector('[data-state="taken"]')).toBeNull();
  });

  it("says the shortcut is taken, and offers a rebind, when another app holds it", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: false,
      unsupported: false,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);

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
      unsupported: false,
    });
    h.setGlobalHotkey.mockResolvedValue(given("CmdOrCtrl+Alt+Space"));
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);

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
      unsupported: false,
    });
    h.setGlobalHotkey.mockResolvedValue(given("CmdOrCtrl+Alt+Space"));
    h.saveConfig.mockClear();
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);

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
    ["Home", "CmdOrCtrl+Shift+Home"],
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
        "Writ can't use that key. Pick a letter",
      );
      expect(h.saveConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          keybindings: { "note.rename": "CmdOrCtrl+Alt+R" },
          hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
        }),
      );
    });
  }

  it("keeps the old chord, writes the keybindings and names the refused chord, when another app holds it", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
      unsupported: false,
    });
    h.setGlobalHotkey.mockResolvedValue({
      status: { chord: "CmdOrCtrl+Shift+Space", registered: true, unsupported: false },
      refused: true,
    });
    h.saveConfig.mockClear();
    await hotkeyStore.load();
    registerRenameCommand();

    const { container } = render(() => <ShortcutEditor />);

    fireEvent.click(container.querySelector('[data-action="record-shortcut"]')!);
    fireEvent.keyDown(document, { key: "r", metaKey: true, altKey: true });

    fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
    fireEvent.keyDown(document, { key: " ", metaKey: true, altKey: true });

    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await settle();

    expect(h.saveConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        keybindings: { "note.rename": "CmdOrCtrl+Alt+R" },
        hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
      }),
    );
    expect(hotkeyStore.isTaken(), "the old chord still works").toBe(false);
    expect(hotkeyStore.chord()).toBe("CmdOrCtrl+Shift+Space");
    expect(row(container).querySelector('[data-state="refused"]')?.textContent).toBe(
      `Another app is using ${formatKeybinding("CmdOrCtrl+Alt+Space")}.`,
    );
  });

  it("refuses a key with no Cmd, Ctrl or Alt, states the rule and keeps listening", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
      unsupported: false,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);

    fireEvent.click(row(container).querySelector('[data-action="record-global-shortcut"]')!);
    fireEvent.keyDown(document, { key: "a", code: "KeyA" });

    const rule = row(container).querySelector('[data-state="rule"]');
    expect(rule?.textContent).toBe(windowChordRule(detectPlatform()));
    expect(row(container).textContent).toContain("Press a key");

    fireEvent.keyDown(document, { key: "A", code: "KeyA", shiftKey: true });
    expect(row(container).querySelector('[data-state="rule"]'), "Shift alone is no modifier here").not.toBeNull();
    expect(row(container).textContent).toContain("Press a key");

    fireEvent.keyDown(document, { key: "F13", code: "F13" });
    expect(row(container).querySelector('[data-state="rule"]')).toBeNull();
    expect(row(container).textContent).toContain(chordAsShown("F13"));
  });

  it("puts the shipped chord back on Reset, whatever was saved before", async () => {
    h.toggle = "Ctrl+Alt+Space";
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "Ctrl+Alt+Space",
      registered: true,
      unsupported: false,
    });
    h.setGlobalHotkey.mockResolvedValue(given("CmdOrCtrl+Shift+Space"));
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    const reset = row(container).querySelector<HTMLButtonElement>(
      '[data-action="reset-global-shortcut"]',
    )!;
    expect(reset.disabled, "a saved chord other than the shipped one can be reset").toBe(false);

    fireEvent.click(reset);
    expect(row(container).textContent).toContain(chordAsShown("CmdOrCtrl+Shift+Space"));
    expect(reset.disabled).toBe(true);

    fireEvent.click(container.querySelector('[data-action="save-shortcuts"]')!);
    await settle();
    expect(h.setGlobalHotkey).toHaveBeenCalledWith("CmdOrCtrl+Shift+Space");
  });

  it("puts the shipped chord back on the window row on Reset all", async () => {
    h.toggle = "Ctrl+Alt+Space";
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "Ctrl+Alt+Space",
      registered: true,
      unsupported: false,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    fireEvent.click(container.querySelector('[data-action="reset-all-shortcuts"]')!);
    expect(row(container).textContent).toContain(chordAsShown("CmdOrCtrl+Shift+Space"));
  });

  // A saved chord the rule refuses falls back to the shipped one at startup,
  // and the row opens on the fallback: that alone is no change to discard.
  it("holds nothing unsaved when the only difference is a refused saved chord", async () => {
    h.toggle = "Shift+A";
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
      unsupported: false,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);
    expect(row(container).textContent).toContain(chordAsShown("CmdOrCtrl+Shift+Space"));
    expect(hasUnsavedShortcuts()).toBe(false);
  });

  it("says a Wayland session cannot pass the chord on, rather than calling it taken", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
      unsupported: true,
    });
    await hotkeyStore.load();

    const { container } = render(() => <ShortcutEditor />);

    expect(row(container).querySelector('[data-state="unsupported"]')?.textContent).toBe(
      WAYLAND_LINE,
    );
    expect(row(container).querySelector('[data-state="taken"]')).toBeNull();
  });

  // `hotkey:status` is emitted by the Rust side, not by the list, so the chord
  // can move while the editor is up. It used to re-seed every row from the
  // config, which took recorded but unsaved chords down with it.
  it("keeps the recorded rows when the chord moves while the list is open", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
      unsupported: false,
    });
    h.setGlobalHotkey.mockResolvedValue(given("CmdOrCtrl+Alt+Space"));
    await hotkeyStore.load();
    registerRenameCommand();

    const { container } = render(() => <ShortcutEditor />);

    fireEvent.click(container.querySelector('[data-action="record-shortcut"]')!);
    fireEvent.keyDown(document, { key: "r", metaKey: true, altKey: true });
    expect(ordinaryRowChord(container)).toBe(chordAsShown("CmdOrCtrl+Alt+R"));

    // Nothing in the list asked for this: it is the answer arriving on its own.
    await hotkeyStore.rebind("CmdOrCtrl+Alt+Space");
    await settle();

    expect(
      ordinaryRowChord(container),
      "a status from outside the list must not reach the other rows",
    ).toBe(chordAsShown("CmdOrCtrl+Alt+R"));
    expect(
      row(container).textContent,
      "the row the status is about takes the chord the OS now holds",
    ).toContain(chordAsShown("CmdOrCtrl+Alt+Space"));
  });

  // Settings, General records the chord straight to the OS and the config. The
  // list, seeded on an earlier visit, must follow it rather than hold the old
  // chord as a change nobody made.
  it("leaves nothing unsaved when the chord moves while the list is off screen", async () => {
    h.globalHotkeyStatus.mockResolvedValue({
      chord: "CmdOrCtrl+Shift+Space",
      registered: true,
      unsupported: false,
    });
    await hotkeyStore.load();
    const { unmount } = render(() => <ShortcutEditor />);
    unmount();

    h.setGlobalHotkey.mockResolvedValue(given("CmdOrCtrl+Alt+Space"));
    await hotkeyStore.rebind("CmdOrCtrl+Alt+Space");
    h.toggle = "CmdOrCtrl+Alt+Space";
    await settle();

    expect(hasUnsavedShortcuts()).toBe(false);
  });
});
