import type { Command as CMCommand } from "@codemirror/view";
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
import { EDITOR_COMMAND_KEYS, type EditorCommandKey } from "./editor-command-keys";

export type { EditorCommandKey } from "./editor-command-keys";
export { EDITOR_COMMAND_KEYS } from "./editor-command-keys";

export interface EditorCommandSpec extends EditorCommandKey {
  run: CMCommand;
}

const RUN_BY_ID: Record<string, CMCommand> = {
  "editor.duplicateLine": duplicateSelectionOrLine,
  "editor.deleteLine": deleteLine,
  "editor.moveLineUp": moveLineUp,
  "editor.moveLineDown": moveLineDown,
  "editor.toggleComment": toggleComment,
  "editor.selectLine": selectLine,
  "editor.insertLineBelow": insertBlankLine,
  "editor.insertLineAbove": insertBlankLineAbove,
  "editor.joinLines": joinLines,
  "editor.selectNextOccurrence": selectNextOccurrence,
};

// Binds each pure key row to its CM command, keeping ids/labels/chords sourced
// only from editor-command-keys.ts.
export const EDITOR_COMMANDS: readonly EditorCommandSpec[] = EDITOR_COMMAND_KEYS.map((key) => {
  const run = RUN_BY_ID[key.id];
  if (!run) throw new Error(`editor-command-table: no CM command for ${key.id}`);
  return { ...key, run };
});

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
