import { rankSettings, SECTION_LABELS } from "../../settings";
import { isSettingAvailable } from "../../settings/availability";
import { openSettings } from "../../components/SettingsModal/SettingsModal";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";

export interface SettingsProviderOptions {
  order?: number;
  cap?: number;
}

export function createSettingsProvider(options: SettingsProviderOptions = {}): ResultProvider {
  return {
    id: "settings",
    section: "Settings",
    order: options.order ?? 1,
    cap: options.cap ?? Number.POSITIVE_INFINITY,
    showKbd: true,
    query(q: string): PaletteResult[] {
      if (!q) return [];
      return rankSettings(q)
        .filter((entry) => isSettingAvailable(entry.id))
        .map((entry) => ({
          id: `setting:${entry.id}`,
          label: entry.title,
          detail: `${SECTION_LABELS[entry.section]} settings`,
          execute: () => openSettings(entry.section, entry.id),
        }));
    },
  };
}
