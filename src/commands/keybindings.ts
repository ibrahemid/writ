import {
  executeCommand,
  getAllCommands,
  notifyRegistryChanged,
  registryVersion,
} from "./registry";
import type { Command } from "../types/commands";
import { isModalOpen } from "../lib/modal-stack";

let keybindingOverrides: Readonly<Record<string, string>> = {};

export function setKeybindingOverrides(overrides: Readonly<Record<string, string>>) {
  keybindingOverrides = overrides;
}

export function effectiveBinding(commandId: string, fallback?: string): string | undefined {
  const override = keybindingOverrides[commandId];
  if (override !== undefined) return override === "" ? undefined : override;
  return fallback;
}

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

export function useEffectiveBinding(commandId: string, fallback?: string): string | undefined {
  registryVersion();
  return effectiveBinding(commandId, fallback);
}

const LEGACY_DEFAULT_BINDINGS: ReadonlyMap<string, string> = new Map([
  ["buffer.new", "CmdOrCtrl+N"],
  ["buffer.close", "CmdOrCtrl+W"],
  ["history.restoreLast", "CmdOrCtrl+Shift+T"],
  ["sidebar.toggle", "CmdOrCtrl+S"],
  ["palette.open", "CmdOrCtrl+Shift+P"],
]);

export function pruneLegacyDefaultOverrides(
  loaded: Readonly<Record<string, string>>,
): Record<string, string> {
  const result: Record<string, string> = {};
  let changed = false;
  for (const [id, binding] of Object.entries(loaded)) {
    if (LEGACY_DEFAULT_BINDINGS.get(id) === binding) {
      changed = true;
      continue;
    }
    result[id] = binding;
  }
  return changed ? result : { ...loaded };
}

export function normalizeKey(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("CmdOrCtrl");
  if (e.shiftKey && e.key !== "Shift") parts.push("Shift");
  if (e.altKey) parts.push("Alt");

  let key = e.key;
  if (key === " ") key = "Space";
  if (key.length === 1) key = key.toUpperCase();
  parts.push(key);

  return parts.join("+");
}

const keyBindingMap = new Map<string, string>();

/**
 * Command ids that may fire while focus is inside the editor or a text input:
 * every `scope: "editor"` command (it is meant for the focused editor) plus any
 * app command flagged `global`. Other app commands are suppressed while typing
 * so their chord reaches the editor/input instead.
 */
const focusPassthrough = new Set<string>();

let lastShiftTime = 0;
let shiftCommandId: string | null = null;

export function rebuildKeyMap() {
  keyBindingMap.clear();
  focusPassthrough.clear();
  shiftCommandId = null;

  for (const cmd of getAllCommands()) {
    if (cmd.scope === "editor" || cmd.global) focusPassthrough.add(cmd.id);

    const primary = effectiveBinding(cmd.id, cmd.keybinding);
    const bindings = [primary, ...(cmd.keybindingAliases ?? [])].filter(
      (b): b is string => Boolean(b),
    );
    for (const binding of bindings) {
      if (binding === "Shift+Shift") {
        shiftCommandId = cmd.id;
        continue;
      }
      keyBindingMap.set(binding, cmd.id);
    }
  }

  if (import.meta.env.DEV) {
    for (const [chord, ids] of findKeybindingConflicts(getAllCommands())) {
      console.warn(`keybinding conflict: ${chord} claimed by ${ids.join(", ")}`);
    }
  }

  notifyRegistryChanged();
}

/**
 * True when keyboard focus is inside a CodeMirror editor or any native text
 * input, where app-scoped chords must yield to the editor unless the matched
 * command is on the focus allowlist. Lives in `src/commands/` (not a component
 * or store), so reading `document.activeElement` here is permitted.
 */
function isTextEntryFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  if (el.closest(".cm-editor")) return true;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return true;
  return (el as HTMLElement).isContentEditable === true;
}

export function handleKeyDown(e: KeyboardEvent): boolean {
  if (isModalOpen()) return false;
  if (e.key === "Shift" && shiftCommandId) {
    const now = Date.now();
    if (now - lastShiftTime < 400) {
      e.preventDefault();
      e.stopPropagation();
      executeCommand(shiftCommandId);
      lastShiftTime = 0;
      return true;
    }
    lastShiftTime = now;
    return false;
  }

  if (e.key !== "Shift") {
    lastShiftTime = 0;
  }

  const normalized = normalizeKey(e);
  const commandId = keyBindingMap.get(normalized);
  if (commandId) {
    if (isTextEntryFocused() && !focusPassthrough.has(commandId)) {
      // Let the keystroke reach the focused editor/input untouched.
      return false;
    }
    e.preventDefault();
    e.stopPropagation();
    executeCommand(commandId);
    return true;
  }
  return false;
}

export function installKeyboardHandler() {
  document.addEventListener("keydown", handleKeyDown, { capture: true });
}

export function uninstallKeyboardHandler() {
  document.removeEventListener("keydown", handleKeyDown, { capture: true });
}
