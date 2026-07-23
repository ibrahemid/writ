import type { EditorView } from "@codemirror/view";
import { EDITOR_COMMANDS } from "./editor-command-table";
import { registerCommand, unregisterCommand } from "../commands/registry";
import { rebuildKeyMap } from "../commands/keybindings";

export type { EditorCommandSpec } from "./editor-command-table";
export { EDITOR_COMMANDS, OWNED_CM_COMMANDS } from "./editor-command-table";

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
