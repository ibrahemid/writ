import { useCommand } from "../commands/registry";
import { useEffectiveBinding } from "../commands/keybindings";
import { formatKeybinding } from "./keybinding-format";

export function commandBinding(commandId: string): string | undefined {
  return useEffectiveBinding(commandId, useCommand(commandId)?.keybinding);
}

export function commandChord(commandId: string): string {
  return formatKeybinding(commandBinding(commandId));
}

export function withChord(label: string, commandId: string): string {
  const chord = commandChord(commandId);
  return chord ? `${label} ${chord}` : label;
}
