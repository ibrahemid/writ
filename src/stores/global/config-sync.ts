import { configStore } from "./config";
import { themeStore } from "./theme";

/**
 * Reloads config after an external edit of config.toml and re-syncs the theme
 * store with it: without the second half the store keeps the pre-edit theme and
 * appearance, and the next theme save writes those back over the external edit.
 *
 * A failed load leaves the config store on defaults, so the theme store is left
 * alone rather than repainted from them.
 */
export async function syncConfigFromDisk(): Promise<void> {
  const loaded = await configStore.load();
  if (!loaded) return;
  const config = configStore.config();
  themeStore.loadConfig(config.theme, config.appearance);
}
