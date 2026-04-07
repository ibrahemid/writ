import type { Command } from "../types/commands";

const commands = new Map<string, Command>();

export function registerCommand(cmd: Command) {
  commands.set(cmd.id, cmd);
}

export function getCommand(id: string): Command | undefined {
  return commands.get(id);
}

export function getAllCommands(): Command[] {
  return Array.from(commands.values());
}

export function executeCommand(id: string) {
  const cmd = commands.get(id);
  if (cmd) cmd.execute();
}
