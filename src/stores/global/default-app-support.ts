import { createSignal } from "solid-js";
import { fetchDefaultAppTypes, fetchDefaultAppStatus } from "./default-app";
import type { ClaimableType } from "./default-app";

// Singleton state — Writ is single-window. Claimable default-app type ids the
// current platform actually supports (status other than "unsupported"). Probed
// once at startup so the command palette and settings search can offer these
// rows without the Settings modal having mounted.
const [supportedDefaultAppIds, setSupportedDefaultAppIds] = createSignal<ReadonlySet<string>>(
  new Set(),
);

/** True once at least one claimable type resolved as supported on this platform. */
export function hasSupportedDefaultAppTypes(): boolean {
  return supportedDefaultAppIds().size > 0;
}

export function markDefaultAppTypeSupported(typeId: string, supported: boolean): void {
  setSupportedDefaultAppIds((prev) => {
    if (supported === prev.has(typeId)) return prev;
    const next = new Set(prev);
    if (supported) next.add(typeId);
    else next.delete(typeId);
    return next;
  });
}

export function clearDefaultAppSupport(): void {
  setSupportedDefaultAppIds(new Set<string>());
}

/**
 * Resolve which claimable default-app types this platform supports and seed the
 * registry. Safe to call once at app start; failures leave the registry empty
 * (every default-app setting then reads unsupported, which is the safe default).
 */
export async function probeDefaultAppSupport(): Promise<void> {
  let types: ClaimableType[];
  try {
    types = await fetchDefaultAppTypes();
  } catch {
    return;
  }
  await Promise.all(
    types.map(async (t) => {
      try {
        const status = await fetchDefaultAppStatus(t.id);
        markDefaultAppTypeSupported(t.id, status.status !== "unsupported");
      } catch {
        markDefaultAppTypeSupported(t.id, false);
      }
    }),
  );
}
