import {
  executeCommand,
  getAllCommands,
  notifyRegistryChanged,
  registryVersion,
} from "./registry";
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

function normalizeKey(e: KeyboardEvent): string {
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

interface DoubleTap {
  commandId: string;
  singleCommandId: string | null;
  baseKey: string;
  modifier: string;
  normalizedSingle: string;
}

const doubleTaps: DoubleTap[] = [];
const pendingTimers = new Map<string, ReturnType<typeof setTimeout>>();

export function rebuildKeyMap() {
  keyBindingMap.clear();
  shiftCommandId = null;
  doubleTaps.length = 0;
  pendingTimers.clear();

  const doubleTapBindings: { commandId: string; modifier: string; baseKey: string }[] = [];
  const regularBindings: { commandId: string; keybinding: string }[] = [];

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
      const doubleTapMatch = binding.match(/^(.+)\+double\+(.+)$/i);
      if (doubleTapMatch) {
        doubleTapBindings.push({
          commandId: cmd.id,
          modifier: doubleTapMatch[1],
          baseKey: doubleTapMatch[2].toUpperCase(),
        });
      } else {
        regularBindings.push({ commandId: cmd.id, keybinding: binding });
      }
    }
  }

  for (const reg of regularBindings) {
    keyBindingMap.set(reg.keybinding, reg.commandId);
  }

  for (const dt of doubleTapBindings) {
    const normalizedSingle = `${dt.modifier}+${dt.baseKey}`;
    const singleCommandId = keyBindingMap.get(normalizedSingle) || null;
    if (singleCommandId) {
      keyBindingMap.delete(normalizedSingle);
    }
    doubleTaps.push({
      commandId: dt.commandId,
      singleCommandId,
      baseKey: dt.baseKey,
      modifier: dt.modifier,
      normalizedSingle,
    });
  }

  notifyRegistryChanged();
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

  for (const dt of doubleTaps) {
    const hasModifier =
      (dt.modifier === "CmdOrCtrl" && (e.metaKey || e.ctrlKey)) ||
      (dt.modifier === "Shift" && e.shiftKey) ||
      (dt.modifier === "Alt" && e.altKey);

    if (hasModifier && e.key.toUpperCase() === dt.baseKey) {
      e.preventDefault();
      e.stopPropagation();

      const tapKey = dt.normalizedSingle;
      const pending = pendingTimers.get(tapKey);

      if (pending) {
        clearTimeout(pending);
        pendingTimers.delete(tapKey);
        executeCommand(dt.commandId);
      } else {
        const timer = setTimeout(() => {
          pendingTimers.delete(tapKey);
        }, 500);
        pendingTimers.set(tapKey, timer);
      }
      return true;
    }
  }

  const normalized = normalizeKey(e);
  const commandId = keyBindingMap.get(normalized);
  if (commandId) {
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
