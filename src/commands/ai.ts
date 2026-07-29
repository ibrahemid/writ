import { registerCommand, unregisterCommand } from "./registry";
import { windowRegistry } from "../stores/global/window-registry";
import { requestConfirm } from "../components/ConfirmDialog/ConfirmDialog";
import { showToast } from "../components/Notifications/Toast";
import { openSettings } from "../components/SettingsModal/SettingsModal";
import { aiRewriteStore, type AnchoredRange } from "../stores/global/ai-rewrite";
import { aiConnectionStore } from "../stores/global/ai-connection";
import { configStore } from "../stores/global/config";
import { REWRITE_ACTIONS, REWRITE_COMMAND_IDS } from "./rewrite-actions";
import type { AiEndpointState } from "../stores/global/ai-rewrite";
import type { AiAction } from "../services/tauri";

export type { AiAction };

/**
 * Resolves everything that would block a rewrite, in one pass, before any text
 * is sent.
 *
 * The endpoint is resolved in Rust by the same code the request guard uses, so
 * consent is always recorded under the exact host the guard later checks.
 * Consent and key are handled together: a user who has neither is never stopped
 * twice.
 *
 * Returns `true` when the rewrite may proceed.
 */
async function clearBlockersBeforeSending(): Promise<boolean> {
  let endpoint: AiEndpointState;
  try {
    endpoint = await aiRewriteStore.endpointState();
  } catch {
    showToast("Could not read the AI settings.", "error");
    return false;
  }

  if (!endpoint.is_allowed || !endpoint.host) {
    const open = await requestConfirm({
      title: "This base URL cannot be used",
      message: "Use https, or http only for a server on this machine.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.base_url");
    return false;
  }

  if (endpoint.is_hosted && !endpoint.is_consented) {
    const confirmed = await requestConfirm({
      title: `Send text to ${endpoint.host_port ?? endpoint.host}?`,
      message: "Only the text you rewrite is sent. Nothing else leaves your machine.",
      confirmLabel: "Send",
    });
    if (!confirmed) return false;
    try {
      endpoint = await aiRewriteStore.consentHost();
    } catch {
      showToast("Could not record the choice.", "error");
      return false;
    }
  }

  if (endpoint.is_hosted && !endpoint.key_state.is_set) {
    const open = await requestConfirm({
      title: `Add an API key for ${endpoint.host}`,
      message: "The key is kept in your keychain, never in config.toml.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.api_key");
    return false;
  }

  return true;
}

/** Runs a rewrite action from any entry point (palette command, status-bar
 * menu, or the editor context menu), acting on the active editor's selection.
 *
 * `presetRange` lets a caller supply a range captured earlier — the context
 * menu pins the selection when it opens, so an edit while the menu is up cannot
 * silently retarget the rewrite. */
export async function runRewriteAction(action: AiAction, presetRange?: AnchoredRange) {
  const model = configStore.config().ai.model.trim();
  if (!model) {
    const open = await requestConfirm({
      title: "Choose a model",
      message: "No model is set.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.model");
    return;
  }
  // The last probe found the endpoint but not this model — say so instead of
  // failing mid-request.
  const status = aiConnectionStore.status();
  if (status?.kind === "model_missing" && status.detail === model) {
    showToast(`"${model}" is not available. Choose a model in AI settings.`, "info", 5000);
    return;
  }

  const win = windowRegistry.getActive();
  if (!win) return;
  const bufferId = win.editor.currentBufferId();
  if (!bufferId) return;

  const range = presetRange ?? (() => {
    const live = win.editor.getSelectionRange(true);
    return live ? { ...live, bufferId } : null;
  })();
  if (!range) return;
  if (range.text.trim().length === 0) {
    showToast("Select some text to rewrite.", "info");
    return;
  }

  if (!range.usedSelection) {
    const confirmed = await requestConfirm({
      title: "Rewrite the whole document?",
      message: `No text is selected. Send the whole document (${range.text.length} characters) to the model?`,
      confirmLabel: "Send",
    });
    if (!confirmed) return;
  }

  if (!(await clearBlockersBeforeSending())) return;

  aiRewriteStore.start(action, range);
}

let registered = false;

export function registerAiCommands() {
  if (registered) return;
  registered = true;

  for (const action of REWRITE_ACTIONS) {
    registerCommand({
      id: action.commandId,
      label: action.label,
      description: action.description,
      keywords: action.keywords,
      scope: "app",
      execute: () => void runRewriteAction(action.id),
    });
  }
}

export function unregisterAiCommands() {
  if (!registered) return;
  registered = false;
  for (const id of REWRITE_COMMAND_IDS) unregisterCommand(id);
}
