import type { Theme } from "../../types/theme";
import warpDark from "./warp-dark.json";
import warpLight from "./warp-light.json";
import tokyoNight from "./tokyo-night.json";
import dracula from "./dracula.json";
import solarizedDark from "./solarized-dark.json";
import catppuccinMocha from "./catppuccin-mocha.json";

const presets: Theme[] = [
  warpDark as Theme,
  warpLight as Theme,
  tokyoNight as Theme,
  dracula as Theme,
  solarizedDark as Theme,
  catppuccinMocha as Theme,
];

export const PRESETS: Readonly<Theme[]> = presets;

export const DEFAULT_PRESET_ID = "warp-dark";

export function getPreset(id: string): Theme | undefined {
  return presets.find((p) => p.id === id);
}

export function getDefaultPreset(): Theme {
  const fallback = getPreset(DEFAULT_PRESET_ID);
  if (!fallback) throw new Error(`Missing default preset: ${DEFAULT_PRESET_ID}`);
  return fallback;
}
