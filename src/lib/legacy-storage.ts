/**
 * Moves what an earlier build kept under `legacyKey` to `currentKey`, unless
 * something is kept there already, then drops `legacyKey` (ADR-059 decision 12).
 */
export function adoptLegacyStorageKey(legacyKey: string, currentKey: string): void {
  try {
    const legacy = localStorage.getItem(legacyKey);
    if (legacy === null) return;
    if (localStorage.getItem(currentKey) === null) localStorage.setItem(currentKey, legacy);
    localStorage.removeItem(legacyKey);
  } catch {
    // Storage refused (private mode, quota): the old key stays for the next launch.
  }
}
