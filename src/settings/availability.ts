import { hasSupportedDefaultAppTypes } from "../stores/global/default-app-support";
import { configStore } from "../stores/global/config";
import { detectPlatform } from "../lib/platform";
import type { Platform } from "../lib/platform";
import { DEFAULT_APP_SETTING_ID, SETTINGS_INDEX } from "./index";
import type { SettingsSection } from "./index";

/**
 * Settings that exist on some platforms only, and where. The `writ` command is
 * one: Writ links it into /usr/local/bin on macOS and Linux, while the Windows
 * installer puts writ.exe on the PATH itself, so there is nothing to install
 * and the row would offer an action that does nothing. Mirrors the platform
 * gate on `cli_status` in src-tauri/src/commands/cli.rs.
 */
const PLATFORMS_BY_SETTING: Readonly<Record<string, ReadonlyArray<Platform>>> = {
  "files.cli": ["mac", "linux"],
  [DEFAULT_APP_SETTING_ID]: ["mac"],
};

const ENTRIES_BY_ID: ReadonlyMap<string, (typeof SETTINGS_INDEX)[number]> = new Map(
  SETTINGS_INDEX.map((entry) => [entry.id, entry]),
);

/**
 * Whether a setting can currently render. All settings are available except
 * the platform-gated rows (the default-app row, which needs at least one
 * claimable type the platform supports, and the rows listed above) and the
 * rows of apps that are all off, which are not drawn. Reactive: reads the
 * store signals, so callers in tracked scopes update as support is discovered
 * and as apps are switched.
 */
export function isSettingAvailable(id: string): boolean {
  const requires = ENTRIES_BY_ID.get(id)?.requires;
  if (requires && !requires.some((app) => configStore.isAppOn(app))) return false;
  const platforms = PLATFORMS_BY_SETTING[id];
  if (platforms && !platforms.includes(detectPlatform())) return false;
  if (id !== DEFAULT_APP_SETTING_ID) return true;
  return hasSupportedDefaultAppTypes();
}

/**
 * Whether a section holds a row this platform can show. The nav asks this, so a
 * section is never offered with nothing under it: the default-app row is the
 * one Files can lose, because `set_default_app` answers Unsupported off macOS.
 *
 * Platform alone decides. The per-type support registry fills in only once the
 * section has mounted and queried, so gating the nav on it would keep the
 * section from ever mounting.
 */
export function isSectionAvailable(section: SettingsSection): boolean {
  return SETTINGS_INDEX.some((entry) => {
    if (entry.section !== section) return false;
    const platforms = PLATFORMS_BY_SETTING[entry.id];
    return !platforms || platforms.includes(detectPlatform());
  });
}
