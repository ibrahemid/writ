import { describe, it, expect } from "vitest";
import type { Command, KeyBinding } from "@codemirror/view";
import {
  defaultKeymap,
  deleteLine,
  copyLineUp,
  copyLineDown,
  moveLineUp,
  moveLineDown,
  toggleComment,
  insertBlankLine,
  selectAll,
  cursorCharLeft,
} from "@codemirror/commands";
import { stripOwnedBindings } from "../../editor/keymap-filter";
import { OWNED_CM_COMMANDS } from "../../editor/editor-commands";

const OWNED = new Set<Command>(OWNED_CM_COMMANDS);

describe("stripOwnedBindings", () => {
  it("drops exactly the owned commands from defaultKeymap", () => {
    const ownedRunCount = defaultKeymap.filter(
      (b) => b.run !== undefined && OWNED.has(b.run),
    ).length;
    const filtered = stripOwnedBindings(defaultKeymap, OWNED_CM_COMMANDS);
    expect(filtered.length).toBe(defaultKeymap.length - ownedRunCount);
  });

  it("leaves non-owned bindings intact (spot checks)", () => {
    const filtered = stripOwnedBindings(defaultKeymap, OWNED_CM_COMMANDS);
    expect(filtered.some((b) => b.run === selectAll)).toBe(true);
    expect(filtered.some((b) => b.run === cursorCharLeft)).toBe(true);
  });

  it("no owned command survives as a run in the filtered array", () => {
    const filtered = stripOwnedBindings(defaultKeymap, OWNED_CM_COMMANDS);
    for (const owned of [
      deleteLine,
      copyLineUp,
      copyLineDown,
      moveLineUp,
      moveLineDown,
      toggleComment,
      insertBlankLine,
    ]) {
      expect(filtered.some((b) => b.run === owned)).toBe(false);
      expect(filtered.some((b) => b.shift === owned)).toBe(false);
    }
  });

  it("clears an owned shift sub-binding while keeping the entry whose run survives", () => {
    const bindings: KeyBinding[] = [
      { key: "Alt-ArrowUp", run: cursorCharLeft, shift: moveLineUp },
    ];
    const filtered = stripOwnedBindings(bindings, OWNED_CM_COMMANDS);
    expect(filtered).toHaveLength(1);
    expect(filtered[0].run).toBe(cursorCharLeft);
    expect(filtered[0].shift).toBeUndefined();
  });
});
