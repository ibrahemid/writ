import {
  executeCommand,
  getAllCommands,
  getCommand,
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

let lastShiftTime = 0;
let shiftCommandId: string | null = null;

export function rebuildKeyMap() {
  keyBindingMap.clear();
  shiftCommandId = null;

  for (const cmd of getAllCommands()) {
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
 * True when keyboard focus is inside a CodeMirror editor. Editor-scoped
 * commands fire only here. Lives in `src/commands/` (not a component or store),
 * so reading `document.activeElement` here is permitted.
 */
function isEditorFocused(): boolean {
  const el = document.activeElement;
  return el !== null && el.closest(".cm-editor") !== null;
}

/**
 * True when keyboard focus is inside a CodeMirror editor or any native text
 * input, where a non-global app-scoped chord must yield to the editor/input.
 * Lives in `src/commands/` (not a component or store), so reading
 * `document.activeElement` here is permitted.
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
  // Composition and dead keys never match a chord: while an IME is composing,
  // the browser reports keyCode 229 and sets isComposing, and the pending key
  // is meant for the input method, not the command map.
  if (e.isComposing || e.keyCode === 229) return false;
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
  if (!commandId) return false;
  const cmd = getCommand(commandId);
  if (!cmd) return false;

  // Scope gate: an editor command runs only with focus inside a CodeMirror
  // editor; a non-global app command yields to any focused text entry; a
  // global app command runs anywhere.
  if (cmd.scope === "editor") {
    if (!isEditorFocused()) return false;
  } else if (!cmd.global && isTextEntryFocused()) {
    return false;
  }

  // Run before deciding: a command that declines (returns false) is a
  // fall-through, so the keystroke reaches CodeMirror and the browser exactly
  // as a native keymap miss would — no preventDefault, no stopPropagation.
  if (!executeCommand(commandId)) return false;
  e.preventDefault();
  e.stopPropagation();
  return true;
}

export function installKeyboardHandler() {
  document.addEventListener("keydown", handleKeyDown, { capture: true });
}

export function uninstallKeyboardHandler() {
  document.removeEventListener("keydown", handleKeyDown, { capture: true });
}
