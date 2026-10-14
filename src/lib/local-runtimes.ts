import type { LocalProbe } from "../services/tauri";

/** Whether the local runtime `id` answered the probe, or `null` when the
 * probe holds no answer for that id. The probe is keyed by provider id. */
export function runtimeRunning(probe: LocalProbe | null, id: string): boolean | null {
  if (!probe || !(id in probe)) return null;
  return probe[id as keyof LocalProbe];
}
