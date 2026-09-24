import { createSignal } from "solid-js";
import { registerCommand, unregisterCommand } from "./registry";
import { rebuildKeyMap } from "./keybindings";
import type { Command } from "../types/commands";
import type { AppId } from "../types/config";
import { configStore } from "../stores/global/config";

// Singleton state — Writ is single-window. The commands each app brings, and
// which of them are in the registry now.
const commandsByApp = new Map<AppId, readonly Command[]>();
const registered = new Set<AppId>();
const [definitions, setDefinitions] = createSignal(0);

/**
 * Names the commands an app brings. They enter the registry only while the
 * app is on (`syncAppCommands`), so an app that is off has no palette row
 * and no chord (ADR-042 section 3).
 */
export function defineAppCommands(app: AppId, commands: readonly Command[]): void {
  commandsByApp.set(app, commands);
  setDefinitions((n) => n + 1);
}

/** Every command id an app brings, on or off. Usage counts for these are
 * kept while their app is off, so switching it back on ranks them as before. */
export function definedAppCommandIds(): string[] {
  return [...commandsByApp.values()].flat().map((command) => command.id);
}

/**
 * Registers the commands of every app that is on and removes the rest, then
 * rebuilds the key map once if anything moved, so a chord follows its app
 * without a restart. Tracks the definitions, so an effect that calls it
 * before the commands are defined runs again once they are.
 */
export function syncAppCommands(isOn: (app: AppId) => boolean): void {
  definitions();
  let changed = false;
  for (const [app, commands] of commandsByApp) {
    const on = isOn(app);
    if (on === registered.has(app)) continue;
    changed = true;
    if (on) {
      for (const command of commands) registerCommand(command);
      registered.add(app);
    } else {
      for (const command of commands) unregisterCommand(command.id);
      registered.delete(app);
    }
  }
  if (changed) rebuildKeyMap();
}

/** Exported for tests: forgets every definition and registration. */
export function resetAppCommands(): void {
  for (const app of registered) {
    for (const command of commandsByApp.get(app) ?? []) unregisterCommand(command.id);
  }
  registered.clear();
  commandsByApp.clear();
}

/** Whether a surface that lists commands offers `command`: always, unless it
 * belongs to an app that is off. Reactive. */
export function isCommandOffered(command: Command): boolean {
  return command.app === undefined || configStore.isAppOn(command.app);
}
