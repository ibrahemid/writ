import type { Command as CMCommand, EditorView } from "@codemirror/view";
import {
  deleteLine,
  copyLineUp,
  copyLineDown,
  moveLineUp,
  moveLineDown,
  toggleComment,
  insertBlankLine,
  selectLine,
} from "@codemirror/commands";
import { selectNextOccurrence } from "@codemirror/search";
import {
  duplicateSelectionOrLine,
  insertBlankLineAbove,
  joinLines,
} from "./line-ops";
import { registerCommand, unregisterCommand } from "../commands/registry";
import { rebuildKeyMap } from "../commands/keybindings";

export interface EditorCommandSpec {
  id: string;
  label: string;
  description?: string;
  keybinding: string;
  aliases?: string[];
  run: CMCommand;
}

// Chord strings must match `normalizeKey` output exactly: modifier order is
// CmdOrCtrl, Shift, Alt, then the key, and arrows keep their DOM names. The
// `CmdOrCtrl+Shift+/` alias covers layouts where `/` needs Shift; on US layouts
// Shift+/ yields `?`, so the alias is inert there rather than wrong.
export const EDITOR_COMMANDS: readonly EditorCommandSpec[] = [
  { id: "editor.duplicateLine", label: "Duplicate Line", keybinding: "CmdOrCtrl+D", run: duplicateSelectionOrLine },
  { id: "editor.deleteLine", label: "Delete Line", keybinding: "CmdOrCtrl+E", aliases: ["CmdOrCtrl+Shift+K"], run: deleteLine },
  { id: "editor.moveLineUp", label: "Move Line Up", keybinding: "Shift+Alt+ArrowUp", run: moveLineUp },
  { id: "editor.moveLineDown", label: "Move Line Down", keybinding: "Shift+Alt+ArrowDown", run: moveLineDown },
  { id: "editor.toggleComment", label: "Toggle Comment", keybinding: "CmdOrCtrl+/", aliases: ["CmdOrCtrl+Shift+/"], run: toggleComment },
  { id: "editor.selectLine", label: "Select Line", keybinding: "CmdOrCtrl+L", run: selectLine },
  { id: "editor.insertLineBelow", label: "Insert Line Below", keybinding: "CmdOrCtrl+Enter", run: insertBlankLine },
  { id: "editor.insertLineAbove", label: "Insert Line Above", keybinding: "CmdOrCtrl+Shift+Enter", run: insertBlankLineAbove },
  { id: "editor.joinLines", label: "Join Lines", keybinding: "CmdOrCtrl+Shift+J", run: joinLines },
  { id: "editor.selectNextOccurrence", label: "Select Next Occurrence", keybinding: "CmdOrCtrl+Shift+D", run: selectNextOccurrence },
];

// CM functions the table takes over, stripped from `defaultKeymap` by identity
// so a runtime rebind cannot desync ownership from views built earlier.
export const OWNED_CM_COMMANDS: readonly CMCommand[] = [
  deleteLine,
  copyLineUp,
  copyLineDown,
  moveLineUp,
  moveLineDown,
  toggleComment,
  insertBlankLine,
];

/**
 * Registers every entry in {@link EDITOR_COMMANDS} with `scope: "editor"` and
 * rebuilds the chord map so a table chord resolves immediately, without relying
 * on a later effect to rebuild. Returns a disposer that unregisters them all
 * and rebuilds again.
 */
export function registerEditorCommands(getView: () => EditorView | null): () => void {
  for (const spec of EDITOR_COMMANDS) {
    registerCommand({
      id: spec.id,
      label: spec.label,
      description: spec.description,
      keybinding: spec.keybinding,
      keybindingAliases: spec.aliases ? [...spec.aliases] : undefined,
      scope: "editor",
      execute: () => {
        const view = getView();
        if (!view) return false;
        return spec.run(view);
      },
    });
  }
  rebuildKeyMap();

  return () => {
    for (const spec of EDITOR_COMMANDS) unregisterCommand(spec.id);
    rebuildKeyMap();
  };
}
