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

export function executeCommand(id: string) {
  const cmd = commands.get(id);
  if (!cmd) return;
  cmd.execute();
  if (onExecute) onExecute(id);
}
