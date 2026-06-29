import { isDefaultAppTypeSupported } from "../stores/global/default-app-support";
import { DEFAULT_APP_SETTING_PREFIX } from "./index";

/**
 * Whether a setting can currently render on this platform. All settings are
 * available except the platform-gated default-app rows, whose support is
 * resolved into the default-app store at startup. Reactive: reads the store
 * signal, so callers in tracked scopes update as support is discovered.
 */
export function isSettingAvailable(id: string): boolean {
  if (!id.startsWith(DEFAULT_APP_SETTING_PREFIX)) return true;
  return isDefaultAppTypeSupported(id.slice(DEFAULT_APP_SETTING_PREFIX.length));
}
