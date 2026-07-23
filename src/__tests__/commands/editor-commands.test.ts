import { describe, it, expect } from "vitest";
import { EDITOR_COMMANDS } from "../../editor/editor-commands";
import { findKeybindingConflicts, normalizeKey } from "../../commands/keybindings";
import type { Command } from "../../types/commands";

function stub(id: string, keybinding: string, aliases?: string[]): Command {
  return {
    id,
    label: id,
    scope: "app",
    keybinding,
    keybindingAliases: aliases,
    execute: () => {},
  };
}

// Editor line/format commands introduced or owned here.
const EDITOR_AS_COMMANDS: Command[] = EDITOR_COMMANDS.map((spec) =>
  stub(spec.id, spec.keybinding, spec.aliases ? [...spec.aliases] : undefined),
);

const FORMAT_COMMANDS: Command[] = [
  stub("editor.toggleBold", "CmdOrCtrl+B"),
  stub("editor.toggleItalic", "CmdOrCtrl+I"),
  stub("editor.toggleStrikethrough", "CmdOrCtrl+Shift+X"),
  stub("editor.toggleInlineCode", "CmdOrCtrl+Shift+E"),
  stub("editor.insertLink", "CmdOrCtrl+K"),
];

// Every app/editor/preview chord registered outside this PR, mirroring the live
// table so the boot-time conflict check is proven silent. Cmd+E is intentionally
// absent: it now belongs to editor.deleteLine (inline code moved to Cmd+Shift+E).
const OTHER_REGISTERED: Command[] = [
  stub("editor.addCursorUp", "Alt+ArrowUp"),
  stub("editor.addCursorDown", "Alt+ArrowDown"),
  stub("editor.find", "CmdOrCtrl+F"),
  stub("editor.findNext", "CmdOrCtrl+G"),
  stub("editor.findPrev", "CmdOrCtrl+Shift+G"),
  stub("editor.replace", "CmdOrCtrl+R", ["CmdOrCtrl+Alt+F"]),
  stub("editor.zoomIn", "CmdOrCtrl+=", ["CmdOrCtrl+Shift++"]),
  stub("editor.zoomOut", "CmdOrCtrl+-"),
  stub("editor.zoomReset", "CmdOrCtrl+0"),
  stub("preview.resetRatio", "CmdOrCtrl+Shift+0"),
  stub("buffer.new", "CmdOrCtrl+T"),
  stub("buffer.newWindow", "CmdOrCtrl+N"),
  stub("buffer.open", "CmdOrCtrl+O"),
  stub("buffer.close", "CmdOrCtrl+W"),
  stub("tab.prev", "CmdOrCtrl+["),
  stub("tab.next", "CmdOrCtrl+]"),
  stub("tab.rename", "F2", ["CmdOrCtrl+Shift+S"]),
  stub("sidebar.toggle", "CmdOrCtrl+S"),
  stub("settings.open", "CmdOrCtrl+,"),
  stub("history.restoreLast", "CmdOrCtrl+Shift+T"),
  stub("palette.open", "Shift+Shift"),
  stub("app.a", "CmdOrCtrl+A"),
  stub("app.one", "CmdOrCtrl+1"),
  stub("app.h", "CmdOrCtrl+H"),
  stub("app.j", "CmdOrCtrl+J"),
  stub("app.backslash", "CmdOrCtrl+Shift+\\"),
  stub("app.shiftR", "CmdOrCtrl+Shift+R"),
  stub("app.shiftV", "CmdOrCtrl+Shift+V"),
  stub("app.escape", "Escape"),
  stub("app.f5", "F5"),
];

// Rebuild the exact DOM key event that produces `chord`, so a chord string in
// the table is proven to be what `normalizeKey` yields, not a hand-typed guess.
function eventForChord(chord: string): KeyboardEvent {
  const parts = chord.split("+");
  const key = parts[parts.length - 1];
  const mods = new Set(parts.slice(0, -1));
  return {
    key: key.length === 1 ? key.toLowerCase() : key,
    metaKey: mods.has("CmdOrCtrl"),
    ctrlKey: false,
    shiftKey: mods.has("Shift"),
    altKey: mods.has("Alt"),
  } as unknown as KeyboardEvent;
}

describe("EDITOR_COMMANDS", () => {
  it("gives every entry a callable run", () => {
    for (const spec of EDITOR_COMMANDS) {
      expect(typeof spec.run).toBe("function");
    }
  });

  it("has no keybinding conflict across the whole registered set (app + editor + format + preview)", () => {
    const all = [...EDITOR_AS_COMMANDS, ...FORMAT_COMMANDS, ...OTHER_REGISTERED];
    const conflicts = findKeybindingConflicts(all);
    expect([...conflicts.keys()]).toEqual([]);
  });

  it("round-trips every chord through normalizeKey from a synthesized event", () => {
    for (const spec of EDITOR_COMMANDS) {
      const chords = [spec.keybinding, ...(spec.aliases ?? [])];
      for (const chord of chords) {
        expect(normalizeKey(eventForChord(chord))).toBe(chord);
      }
    }
  });

  it("normalizes the Shift+/ alias per layout: '/' keeps the alias, '?' does not", () => {
    // Layout where Shift+/ yields '/': the alias matches.
    expect(
      normalizeKey({
        key: "/",
        metaKey: true,
        ctrlKey: false,
        shiftKey: true,
        altKey: false,
      } as unknown as KeyboardEvent),
    ).toBe("CmdOrCtrl+Shift+/");
    // US layout where Shift+/ yields '?': the primary alias is inert.
    expect(
      normalizeKey({
        key: "?",
        metaKey: true,
        ctrlKey: false,
        shiftKey: true,
        altKey: false,
      } as unknown as KeyboardEvent),
    ).toBe("CmdOrCtrl+Shift+?");
  });
});
