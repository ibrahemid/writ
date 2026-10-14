import { getCommand, registryVersion } from "./registry";

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

/**
 * The chord a command answers to now: the saved override, else the chord it
 * registered with. Reactive on the registry, which every key map rebuild
 * bumps, so a hint redraws when a shortcut is saved.
 */
export function useCommandBinding(commandId: string): string | undefined {
  registryVersion();
  return effectiveBinding(commandId, getCommand(commandId)?.keybinding);
}
