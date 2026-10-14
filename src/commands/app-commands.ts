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

/** What a command of an app that is off does: opens that app's switch. */
let answerWhileOff: (app: AppId) => void = () => {};

/**
 * Names what a command of an app that is off does when it runs, which is
 * what its chord does while the app is off (ADR-042 section 3). Set once by
 * the window's owner, which can reach Settings; this module cannot, because
 * Settings lists the commands this module registers.
 */
export function answerOffAppCommands(open: (app: AppId) => void): void {
  answerWhileOff = open;
}

/**
 * Names the commands an app brings. They stay in the registry whether the
 * app is on or off, stamped with the app, so the palette, both menus and the
 * shortcut editor leave them out while it is off (`isCommandOffered`) and a
 * chord bound to one opens the app's switch instead of doing nothing.
 */
export function defineAppCommands(app: AppId, commands: readonly Command[]): void {
  if (registered.delete(app)) {
    for (const command of commandsByApp.get(app) ?? []) unregisterCommand(command.id);
  }
  commandsByApp.set(app, commands);
  setDefinitions((n) => n + 1);
}

/** Every command id an app brings, on or off. Usage counts for these are
 * kept while their app is off, so switching it back on ranks them as before. */
export function definedAppCommandIds(): string[] {
  return [...commandsByApp.values()].flat().map((command) => command.id);
}

/** `command` as the registry holds it: its app's, and its app's switch while off. */
function stamped(app: AppId, command: Command): Command {
  return {
    ...command,
    app,
    execute: () => {
      if (configStore.isAppOn(app)) return command.execute();
      answerWhileOff(app);
    },
  };
}

/**
 * Registers the commands of every app defined since the last call, then
 * rebuilds the key map once if anything was added, so a chord answers as soon
 * as its app's commands exist. Tracks the definitions, so an effect that
 * calls it before the commands are defined runs again once they are.
 */
export function syncAppCommands(): void {
  definitions();
  let changed = false;
  for (const [app, commands] of commandsByApp) {
    if (registered.has(app)) continue;
    for (const command of commands) registerCommand(stamped(app, command));
    registered.add(app);
    changed = true;
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
  answerWhileOff = () => {};
}

/** Whether a surface that lists commands offers `command`: always, unless it
 * belongs to an app that is off. Reactive. */
export function isCommandOffered(command: Command): boolean {
  return command.app === undefined || configStore.isAppOn(command.app);
}
