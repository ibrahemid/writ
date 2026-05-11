import { executeCommand, getAllCommands } from "./registry";

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
    const bindings = [cmd.keybinding, ...(cmd.keybindingAliases ?? [])].filter(
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
}

export function handleKeyDown(e: KeyboardEvent): boolean {
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
