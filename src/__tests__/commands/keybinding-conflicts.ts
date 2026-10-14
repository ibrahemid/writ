import { effectiveBinding } from "../../commands/keybindings";
import type { Command } from "../../types/commands";

/**
 * Report chords claimed by more than one command. A command's own primary and
 * alias bindings do not conflict with each other; only collisions across
 * different commands are returned, keyed by chord to the conflicting ids.
 */
export function findKeybindingConflicts(commands: Command[]): Map<string, string[]> {
  const owners = new Map<string, string[]>();
  for (const cmd of commands) {
    const primary = effectiveBinding(cmd.id, cmd.keybinding);
    const bindings = [primary, ...(cmd.keybindingAliases ?? [])].filter(
      (b): b is string => Boolean(b),
    );
    for (const binding of new Set(bindings)) {
      const ids = owners.get(binding) ?? [];
      ids.push(cmd.id);
      owners.set(binding, ids);
    }
  }
  const conflicts = new Map<string, string[]>();
  for (const [binding, ids] of owners) {
    if (ids.length > 1) conflicts.set(binding, ids);
  }
  return conflicts;
}
