import { createSignal } from "solid-js";
import type { Command } from "../types/commands";

const commands = new Map<string, Command>();
const [version, setVersion] = createSignal(0);

let onExecute: ((id: string) => void) | null = null;

function bump() {
  setVersion((v) => v + 1);
}

export function registerCommand(cmd: Command) {
  commands.set(cmd.id, cmd);
  bump();
}

export function unregisterCommand(id: string) {
  if (commands.delete(id)) bump();
}

export function getCommand(id: string): Command | undefined {
  return commands.get(id);
}

export function getAllCommands(): Command[] {
  return Array.from(commands.values());
}

export function useCommand(id: string): Command | undefined {
  version();
  return commands.get(id);
}

export function useAllCommands(): Command[] {
  version();
  return Array.from(commands.values());
}

export function registryVersion(): number {
  return version();
}

export function notifyRegistryChanged() {
  bump();
}

export function setExecuteListener(listener: ((id: string) => void) | null) {
  onExecute = listener;
}

/**
 * Runs a command by id and reports whether it handled the keystroke. Returns
 * `true` when the command returned anything other than `false` (a throwing
 * command also counts as handled, so an exception never falls through into the
 * document as an un-prevented keystroke); returns `false` for an unknown id or
 * a command that returned `false`. The execute listener is notified only when
 * the command actually ran.
 */
export function executeCommand(id: string): boolean {
  const cmd = commands.get(id);
  if (!cmd) return false;
  let result: boolean | void | Promise<unknown>;
  try {
    result = cmd.execute();
  } catch (err) {
    console.error(`command "${id}" threw during execution`, err);
    return true;
  }
  if (result === false) return false;
  if (onExecute) onExecute(id);
  return true;
}
