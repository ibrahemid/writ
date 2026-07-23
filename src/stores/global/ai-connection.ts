import { createSignal, createRoot } from "solid-js";
import { aiCheckConnection, type AiConnectionStatus } from "../../services/tauri";
import { configStore } from "./config";

export type { AiConnectionStatus };

export type ConnectionTone = "ok" | "warn" | "error" | "idle";

export interface ConnectionDisplay {
  text: string;
  tone: ConnectionTone;
}

function baseUrlIsLocal(): boolean {
  try {
    const host = new URL(configStore.config().ai.base_url).hostname;
    return host === "localhost" || host === "127.0.0.1" || host === "::1" || host === "[::1]";
  } catch {
    return false;
  }
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
    case "model_missing":
      return { text: `Connected, but "${status.detail || model}" is not available`, tone: "warn" };
    case "unauthorized":
      return { text: `Authentication failed (${status.detail}). Check the API key.`, tone: "error" };
    case "server_error":
      return { text: `The server returned status ${status.detail}`, tone: "error" };
    case "refused":
      return baseUrlIsLocal()
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

// Singleton state — Writ is single-window. Holds the latest probe result.
function createAiConnectionStore() {
  const [status, setStatus] = createSignal<AiConnectionStatus | null>(null);
  const [checking, setChecking] = createSignal(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  async function check(): Promise<void> {
    if (!configStore.config().ai.enabled) {
      setStatus(null);
      return;
    }
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

  return { status, checking, check, scheduleCheck, reset };
}

export type AiConnectionStore = ReturnType<typeof createAiConnectionStore>;
export const aiConnectionStore = createRoot(createAiConnectionStore);
