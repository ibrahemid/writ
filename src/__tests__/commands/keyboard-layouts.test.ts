import { describe, it, expect, afterEach, vi } from "vitest";
import { registerCommand, unregisterCommand, getAllCommands } from "../../commands/registry";
import {
  handleKeyDown,
  rebuildKeyMap,
  setKeybindingOverrides,
} from "../../commands/keybindings";

// Chords are matched by the character a key produced, then by the key's
// position, so they hold on non-Latin layouts; on macOS Control is its own
// modifier, so a Control chord stays the editor's.

interface KeyInit {
  key: string;
  code?: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  altGraph?: boolean;
}

function keyEvent(init: KeyInit) {
  return {
    key: init.key,
    code: init.code ?? "",
    metaKey: init.metaKey ?? false,
    ctrlKey: init.ctrlKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
    isComposing: false,
    keyCode: 0,
    getModifierState: (modifier: string) => modifier === "AltGraph" && init.altGraph === true,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  } as unknown as KeyboardEvent & {
    preventDefault: ReturnType<typeof vi.fn>;
    stopPropagation: ReturnType<typeof vi.fn>;
  };
}

function hostIs(platform: string) {
  Object.defineProperty(window.navigator, "platform", { value: platform, configurable: true });
}

const ran: string[] = [];

function command(id: string, keybinding: string, aliases?: string[]) {
  registerCommand({
    id,
    label: id,
    keybinding,
    keybindingAliases: aliases,
    scope: "app",
    global: true,
    execute: () => {
      ran.push(id);
    },
  });
}

afterEach(() => {
  delete (window.navigator as unknown as Record<string, unknown>).platform;
  for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
  setKeybindingOverrides({});
  rebuildKeyMap();
  ran.length = 0;
});

describe("chords on a non-Latin layout", () => {
  it("makes a file on Ctrl+N under an Arabic layout on Windows", () => {
    hostIs("Win32");
    command("note.new", "CmdOrCtrl+N");
    rebuildKeyMap();

    expect(handleKeyDown(keyEvent({ key: "ى", code: "KeyN", ctrlKey: true }))).toBe(true);
    expect(ran).toEqual(["note.new"]);
  });

  it("answers Russian and Greek letters by their position", () => {
    hostIs("Linux x86_64");
    command("search.everywhere", "CmdOrCtrl+Shift+F");
    command("buffer.close", "CmdOrCtrl+W");
    rebuildKeyMap();

    handleKeyDown(keyEvent({ key: "А", code: "KeyF", ctrlKey: true, shiftKey: true }));
    handleKeyDown(keyEvent({ key: "ς", code: "KeyW", ctrlKey: true }));
    expect(ran).toEqual(["search.everywhere", "buffer.close"]);
  });

  it("leaves AltGr alone: Polish AltGr+S types ś rather than toggling the sidebar", () => {
    hostIs("Win32");
    command("sidebar.toggle", "CmdOrCtrl+\\", ["CmdOrCtrl+Alt+S"]);
    rebuildKeyMap();

    const event = keyEvent({ key: "ś", code: "KeyS", ctrlKey: true, altKey: true, altGraph: true });
    expect(handleKeyDown(event)).toBe(false);
    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(ran).toEqual([]);
  });

  it("opens a folder on Ctrl+Alt+O on Windows, and leaves Polish AltGr+O to type ó", () => {
    hostIs("Win32");
    command("workspace.openFolder", "CmdOrCtrl+Alt+O");
    rebuildKeyMap();

    expect(handleKeyDown(keyEvent({ key: "o", code: "KeyO", ctrlKey: true, altKey: true }))).toBe(
      true,
    );
    const polish = keyEvent({ key: "ó", code: "KeyO", ctrlKey: true, altKey: true, altGraph: true });
    expect(handleKeyDown(polish)).toBe(false);
    expect(polish.preventDefault).not.toHaveBeenCalled();
    expect(ran).toEqual(["workspace.openFolder"]);
  });

  it("keeps a punctuation alias on its character: German Shift+7 is the slash", () => {
    hostIs("Win32");
    command("editor.toggleComment", "CmdOrCtrl+/", ["CmdOrCtrl+Shift+/"]);
    command("digit.seven", "CmdOrCtrl+Shift+7");
    rebuildKeyMap();

    handleKeyDown(keyEvent({ key: "/", code: "Digit7", ctrlKey: true, shiftKey: true }));
    expect(ran).toEqual(["editor.toggleComment"]);
  });

  it("reaches a shifted digit chord the character misses", () => {
    hostIs("MacIntel");
    command("preview.resetRatio", "CmdOrCtrl+Shift+0");
    rebuildKeyMap();

    handleKeyDown(keyEvent({ key: ")", code: "Digit0", metaKey: true, shiftKey: true }));
    expect(ran).toEqual(["preview.resetRatio"]);
  });

  it("keeps the printed letter on AZERTY: the key labelled A is never Q", () => {
    hostIs("MacIntel");
    command("quit.like", "CmdOrCtrl+Q");
    rebuildKeyMap();

    expect(handleKeyDown(keyEvent({ key: "a", code: "KeyQ", metaKey: true }))).toBe(false);
    expect(ran).toEqual([]);
  });
});

describe("Control and Command on macOS", () => {
  it("lets Ctrl+N through to the editor, where it moves down a line", () => {
    hostIs("MacIntel");
    command("note.new", "CmdOrCtrl+N");
    rebuildKeyMap();

    const event = keyEvent({ key: "n", code: "KeyN", ctrlKey: true });
    expect(handleKeyDown(event)).toBe(false);
    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(event.stopPropagation).not.toHaveBeenCalled();
    expect(ran).toEqual([]);
  });

  it("lets Ctrl+W and Ctrl+K through as well", () => {
    hostIs("MacIntel");
    command("buffer.close", "CmdOrCtrl+W");
    command("md.link", "CmdOrCtrl+K");
    rebuildKeyMap();

    expect(handleKeyDown(keyEvent({ key: "w", code: "KeyW", ctrlKey: true }))).toBe(false);
    expect(handleKeyDown(keyEvent({ key: "k", code: "KeyK", ctrlKey: true }))).toBe(false);
    expect(ran).toEqual([]);
  });

  it("runs Cmd+N as before", () => {
    hostIs("MacIntel");
    command("note.new", "CmdOrCtrl+N");
    rebuildKeyMap();

    expect(handleKeyDown(keyEvent({ key: "n", code: "KeyN", metaKey: true }))).toBe(true);
    expect(ran).toEqual(["note.new"]);
  });

  it("runs a chord recorded with Control on Control alone", () => {
    hostIs("MacIntel");
    command("sidebar.toggle", "CmdOrCtrl+\\");
    setKeybindingOverrides({ "sidebar.toggle": "Ctrl+K" });
    rebuildKeyMap();

    handleKeyDown(keyEvent({ key: "k", code: "KeyK", metaKey: true }));
    expect(ran).toEqual([]);
    handleKeyDown(keyEvent({ key: "k", code: "KeyK", ctrlKey: true }));
    expect(ran).toEqual(["sidebar.toggle"]);
  });
});

describe("Control elsewhere", () => {
  it("reads a Control chord recorded on a Mac as Ctrl on Windows", () => {
    hostIs("Win32");
    command("sidebar.toggle", "CmdOrCtrl+\\");
    setKeybindingOverrides({ "sidebar.toggle": "Ctrl+K" });
    rebuildKeyMap();

    handleKeyDown(keyEvent({ key: "k", code: "KeyK", ctrlKey: true }));
    expect(ran).toEqual(["sidebar.toggle"]);
  });
});
