import { describe, it, expect } from "vitest";
import { MENU_COMMANDS } from "../../commands/menu-commands";
import { EDITOR_COMMAND_KEYS } from "../../editor/editor-command-keys";
import { APP_REGISTRATIONS, APP_TSX_COUNTS } from "./app-registrations";

// Three tables hand out chords, and until now each was only checked against
// itself: `App.tsx`'s `registerCommand` calls, `EDITOR_COMMAND_KEYS`, and the
// menu list's accelerators. A chord claimed in two of them is a key whose
// behaviour depends on which handler runs first.
//
// This does not cover the Markdown chords (⌘B, ⌘I, ⌘K, ⌘⇧E): those are bound in
// the CodeMirror keymap and are only live while a Markdown file is open.

/** Every chord a table hands out, by the command id that claims it. */
function claims(): Map<string, Set<string>> {
  const byChord = new Map<string, Set<string>>();

  const claim = (chord: string, id: string) => {
    if (!chord) return;
    const ids = byChord.get(chord) ?? new Set<string>();
    ids.add(id);
    byChord.set(chord, ids);
  };

  for (const [id, registration] of APP_REGISTRATIONS) {
    claim(registration.keybinding, id);
    for (const alias of registration.aliases) claim(alias, id);
  }
  for (const command of EDITOR_COMMAND_KEYS) {
    claim(command.keybinding, command.id);
    for (const alias of command.aliases ?? []) claim(alias, command.id);
  }
  for (const entry of MENU_COMMANDS) {
    if (entry.accelerator) claim(entry.accelerator, entry.id);
  }

  return byChord;
}

describe("chords across the command registry, the editor table and the menu list", () => {
  it("gives every chord to one command", () => {
    const contested = [...claims()]
      .filter(([, ids]) => ids.size > 1)
      .map(([chord, ids]) => `${chord}: ${[...ids].sort().join(", ")}`);

    expect(contested, "one chord, two commands").toEqual([]);
  });

  it("reads a chord out of each of the three tables, so an empty run cannot pass", () => {
    const byChord = claims();
    expect(byChord.get("CmdOrCtrl+S"), "the registry").toContain("buffer.save");
    expect(byChord.get("CmdOrCtrl+Shift+K"), "the editor table").toContain("editor.deleteLine");
    expect(byChord.get("CmdOrCtrl+Shift+\\"), "the menu list").toContain("panel.toggle");
  });

  it("reads every registration App.tsx writes, so a silent parse gap cannot pass", () => {
    const withKeybinding = [...APP_REGISTRATIONS.values()].filter(
      (registration) => registration.keybinding.length > 0,
    );
    const withAliases = [...APP_REGISTRATIONS.values()].filter(
      (registration) => registration.aliases.length > 0,
    );

    expect(APP_REGISTRATIONS.size, "commands read").toBe(APP_TSX_COUNTS.registerCommandCalls);
    expect(withKeybinding.length, "chords read").toBe(APP_TSX_COUNTS.keybindingFields);
    expect(withAliases.length, "alias lists read").toBe(APP_TSX_COUNTS.aliasFields);
  });
});
