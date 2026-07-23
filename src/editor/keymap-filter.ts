import type { Command as CMCommand, KeyBinding } from "@codemirror/view";

/**
 * Removes bindings whose `run` is owned by the app's command registry, so a CM
 * keymap (e.g. `defaultKeymap`) cannot double-bind a chord the registry already
 * dispatches. A binding whose `run` is owned is dropped whole; a binding whose
 * `run` survives but whose `shift` sub-binding is owned keeps its entry with
 * the `shift` cleared. Ownership is by function identity, not by chord.
 */
export function stripOwnedBindings(
  bindings: readonly KeyBinding[],
  owned: readonly CMCommand[],
): KeyBinding[] {
  const ownedSet = new Set<CMCommand>(owned);
  const result: KeyBinding[] = [];
  for (const binding of bindings) {
    if (binding.run && ownedSet.has(binding.run)) continue;
    if (binding.shift && ownedSet.has(binding.shift)) {
      const { shift: _shift, ...rest } = binding;
      result.push(rest);
    } else {
      result.push(binding);
    }
  }
  return result;
}
