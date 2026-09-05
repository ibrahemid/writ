import { isDefaultAppTypeSupported } from "../stores/global/default-app-support";
import { detectPlatform } from "../lib/platform";
import type { Platform } from "../lib/platform";
import { DEFAULT_APP_SETTING_PREFIX } from "./index";

/**
 * Settings that exist on some platforms only, and where. The `writ` command is
 * one: Writ links it into /usr/local/bin on macOS and Linux, while the Windows
 * installer puts writ.exe on the PATH itself, so there is nothing to install
 * and the row would offer an action that does nothing. Mirrors the platform
 * gate on `cli_status` in src-tauri/src/commands/cli.rs.
 */
const PLATFORMS_BY_SETTING: Readonly<Record<string, ReadonlyArray<Platform>>> = {
  "files.cli": ["mac", "linux"],
};

/**
 * Whether a setting can currently render on this platform. All settings are
 * available except the platform-gated rows: the default-app rows, whose support
 * is resolved into the default-app store at startup, and the rows listed above.
 * Reactive: reads the store signal, so callers in tracked scopes update as
 * support is discovered.
 */
export function isSettingAvailable(id: string): boolean {
  const platforms = PLATFORMS_BY_SETTING[id];
  if (platforms) return platforms.includes(detectPlatform());
  if (!id.startsWith(DEFAULT_APP_SETTING_PREFIX)) return true;
  return isDefaultAppTypeSupported(id.slice(DEFAULT_APP_SETTING_PREFIX.length));
}
