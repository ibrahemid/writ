import { hasSupportedDefaultAppTypes } from "../stores/global/default-app-support";
import { configStore } from "../stores/global/config";
import { DEFAULT_APP_SETTING_ID, SETTINGS_INDEX } from "./index";
import type { SettingsSection } from "./index";

const ENTRIES_BY_ID: ReadonlyMap<string, (typeof SETTINGS_INDEX)[number]> = new Map(
  SETTINGS_INDEX.map((entry) => [entry.id, entry]),
);

/**
 * Whether a setting can currently render. All settings are available except
 * the default-app row, which needs at least one claimable type the platform
 * supports (Launch Services on macOS, a desktop entry and `xdg-mime` on
 * Linux, the Default apps page on Windows), and the rows of apps that are all
 * off, which are not drawn. Reactive: reads the store signals, so callers in
 * tracked scopes update as support is discovered and as apps are switched.
 */
export function isSettingAvailable(id: string): boolean {
  const requires = ENTRIES_BY_ID.get(id)?.requires;
  if (requires && !requires.some((app) => configStore.isAppOn(app))) return false;
  if (id !== DEFAULT_APP_SETTING_ID) return true;
  return hasSupportedDefaultAppTypes();
}

/**
 * Whether a section holds a row. The nav asks this, so a section is never
 * offered with nothing under it.
 *
 * The per-type support registry fills in only once the section has mounted
 * and queried, so gating the nav on it would keep the section from ever
 * mounting.
 */
export function isSectionAvailable(section: SettingsSection): boolean {
  return SETTINGS_INDEX.some((entry) => entry.section === section);
}
