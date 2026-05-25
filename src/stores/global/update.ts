import { createRoot, createSignal } from "solid-js";
import { onEvent, type UnlistenFn } from "../../services/events";
import {
  checkForUpdate as checkForUpdateIpc,
  downloadAndInstallUpdate,
  dismissUpdate,
  restartApp,
} from "../../services/tauri";
import type { UpdatePhase } from "../../types/update";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// The Rust UpdatePhase is the source of truth; this store mirrors it from
// writ://update-status events and exposes the user actions that drive transitions.

const UNREACHABLE_MESSAGE = "Couldn't reach the update server.";

function createUpdateStore() {
  const [phase, setPhase] = createSignal<UpdatePhase>({ status: "idle" });

  function applyPhase(next: UpdatePhase) {
    setPhase(next);
  }

  async function checkForUpdate() {
    setPhase({ status: "checking" });
    try {
      await checkForUpdateIpc();
    } catch {
      setPhase({ status: "failed", message: UNREACHABLE_MESSAGE });
    }
  }

  async function install() {
    try {
      await downloadAndInstallUpdate();
    } catch {
      setPhase({ status: "failed", message: UNREACHABLE_MESSAGE });
    }
  }

  async function dismiss() {
    setPhase({ status: "idle" });
    try {
      await dismissUpdate();
    } catch {
      // Best-effort: the UI is already hidden; the backend phase is harmless.
    }
  }

  async function restart() {
    await restartApp();
  }

  async function subscribe(): Promise<UnlistenFn> {
    return onEvent("update:status", (next) => applyPhase(next));
  }

  return { phase, applyPhase, checkForUpdate, install, dismiss, restart, subscribe };
}

export const updateStore = createRoot(createUpdateStore);
