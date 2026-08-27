import type { Theme } from "../../types/theme";
import writLight from "./writ-light.json";
import writDark from "./writ-dark.json";
import warpDark from "./warp-dark.json";
import warpLight from "./warp-light.json";
import tokyoNight from "./tokyo-night.json";
import dracula from "./dracula.json";
import solarizedDark from "./solarized-dark.json";
import catppuccinMocha from "./catppuccin-mocha.json";

const presets: Theme[] = [
  writLight as Theme,
  writDark as Theme,
  warpDark as Theme,
  warpLight as Theme,
  tokyoNight as Theme,
  dracula as Theme,
  solarizedDark as Theme,
  catppuccinMocha as Theme,
];

export const PRESETS: Readonly<Theme[]> = presets;

export const DEFAULT_PRESET_ID = "writ-light";

/**
 * Presets that come as a light/dark pair. Following the system setting swaps
 * within a pair; a preset that is not in this map has no counterpart to swap
 * to, so it stays put whatever the system says.
 */
export const PRESET_PAIRS: Readonly<Record<string, { light: string; dark: string }>> = {
  "writ-light": { light: "writ-light", dark: "writ-dark" },
  "writ-dark": { light: "writ-light", dark: "writ-dark" },
  "warp-light": { light: "warp-light", dark: "warp-dark" },
  "warp-dark": { light: "warp-light", dark: "warp-dark" },
};

/** The `want` half of `id`'s pair, or `id` itself when it has no pair. */
export function pairedPreset(id: string, want: "light" | "dark"): string {
  return PRESET_PAIRS[id]?.[want] ?? id;
}

export function getPreset(id: string): Theme | undefined {
  return presets.find((p) => p.id === id);
}

export function getDefaultPreset(): Theme {
  const fallback = getPreset(DEFAULT_PRESET_ID);
  if (!fallback) throw new Error(`Missing default preset: ${DEFAULT_PRESET_ID}`);
  return fallback;
}
