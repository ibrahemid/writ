import { configStore } from "../stores/global/config";

// Shared preview actions used by both the keymap and the in-pane UI, so the
// two never drift.

/**
 * Flip the app-level `preview.run_scripts` kill switch and re-apply it to the
 * active preview. Persists the config (the switch is app-global, not
 * per-buffer) and invokes `onApplied` — typically a force-refresh so the
 * iframe reloads under the new document CSP.
 */
export async function toggleRunScripts(onApplied: () => void): Promise<void> {
  const current = configStore.config();
  await configStore.save({
    ...current,
    preview: { ...current.preview, run_scripts: !current.preview.run_scripts },
  });
  onApplied();
}
