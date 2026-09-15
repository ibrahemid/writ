import { createSignal, createRoot } from "solid-js";
import {
  aiCheckConnection,
  aiListModels,
  aiProbeLocal,
  aiOpenrouterCancel,
  aiOpenrouterConnect,
  type AiConnectionStatus,
  type AiKeyState,
  type LocalProbe,
  type ModelListError,
  type ModelListResult,
} from "../../services/tauri";
import { configStore } from "./config";
import { aiProvidersStore } from "./ai-providers";

export type { AiConnectionStatus, LocalProbe, ModelListError, ModelListResult };

export type ConnectionTone = "ok" | "warn" | "error" | "idle";

export interface ConnectionDisplay {
  text: string;
  tone: ConnectionTone;
}

/** Whether the connection points at a runtime on this machine.
 *
 * Read from the provider table's group rather than parsed out of a URL, so
 * the panel and the endpoint guard cannot disagree about it. An unloaded table
 * answers `false`: naming Ollama in a failure is the wrong guess when the
 * provider is not known to be local. */
function providerIsLocal(): boolean {
  return aiProvidersStore.groupOf(configStore.config().ai.provider) === "local";
}

/** Maps a probe result to a display line and tone. Single source of the copy. */
export function connectionDisplay(
  status: AiConnectionStatus | null,
  model: string,
): ConnectionDisplay {
  if (!status) return { text: "Not checked", tone: "idle" };
  switch (status.kind) {
    case "ok":
      return { text: "Connected", tone: "ok" };
    case "consent_required":
      return { text: `Not checked until you allow ${status.detail}`, tone: "warn" };
    case "key_required":
      return { text: "Add an API key to check the connection.", tone: "warn" };
    case "model_missing":
      return { text: `Connected, but "${status.detail || model}" is not available`, tone: "warn" };
    case "unauthorized":
      return { text: `${status.detail} rejected the API key. Check it.`, tone: "error" };
    case "server_error":
      return { text: `The server returned status ${status.detail}`, tone: "error" };
    case "refused":
      return providerIsLocal()
        ? { text: `Ollama or LM Studio is not running at ${status.detail}`, tone: "error" }
        : { text: `Could not reach ${status.detail}`, tone: "error" };
    case "timeout":
      return { text: `No response from ${status.detail} within 3 seconds`, tone: "error" };
    case "invalid_url":
      return { text: "The base URL is not valid", tone: "error" };
    default:
      return { text: "Could not reach the endpoint", tone: "error" };
  }
}

/** The line a failed model list shows. The error's own text never reaches the
 * panel (ADR-031 rule 5.3): the kind picks a written sentence, and the host
 * comes from the endpoint state the panel already resolved. */
export function modelListDisplay(error: ModelListError, host: string): ConnectionDisplay {
  switch (error.kind) {
    case "unreachable":
      return { text: `Could not reach ${host}`, tone: "error" };
    case "timeout":
      return { text: `No response from ${host} within 5 seconds`, tone: "error" };
    case "unauthorized":
      return { text: `${host} rejected the API key. Check it.`, tone: "error" };
    case "status":
      return { text: `The server returned status ${error.code}`, tone: "error" };
    case "malformed":
      return { text: "The server's model list could not be read", tone: "error" };
    case "consent_required":
      return { text: `Not checked until you allow ${host}`, tone: "warn" };
  }
}

/** The next step after a key is saved and no model is chosen yet. */
export const CHOOSE_A_MODEL = "Your key is saved. Choose a model to finish.";

// Singleton state — Writ is single-window. Holds the latest probe result.
function createAiConnectionStore() {
  const [status, setStatus] = createSignal<AiConnectionStatus | null>(null);
  const [checking, setChecking] = createSignal(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // One connection, checked whether or not either feature is switched on: the
  // check is what tells a person the connection works before they turn
  // anything on with it.
  async function check(): Promise<void> {
    setChecking(true);
    try {
      setStatus(await aiCheckConnection());
    } catch {
      setStatus({ reachable: false, model_listed: null, kind: "error", detail: "", models: [] });
    } finally {
      setChecking(false);
    }
  }

  function scheduleCheck(delayMs = 400): void {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      void check();
    }, delayMs);
  }

  function reset(): void {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    setStatus(null);
  }

  return {
    status,
    checking,
    check,
    scheduleCheck,
    reset,
    /** The models the connection's provider lists, or why it could not be
     * read. */
    listModels: (): Promise<ModelListResult> => aiListModels(),
    /** Which local runtime answered its port. Keyless, and carries no note
     * text (ADR-040 section 4). */
    probeLocal: (): Promise<LocalProbe> => aiProbeLocal(),
    /** Runs the OpenRouter PKCE flow and resolves once the key is stored. */
    connectOpenrouter: (): Promise<AiKeyState> => aiOpenrouterConnect(),
    cancelOpenrouter: (): Promise<void> => aiOpenrouterCancel(),
  };
}

export type AiConnectionStore = ReturnType<typeof createAiConnectionStore>;
export const aiConnectionStore = createRoot(createAiConnectionStore);
