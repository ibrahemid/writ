import { windowRegistry } from "../stores/global/window-registry";
import { requestConfirm } from "../stores/global/dialogs";
import { showToast } from "../stores/global/notifications";
import { openSettings } from "../components/SettingsModal/SettingsModal";
import { aiRewriteStore, type AnchoredRange } from "../stores/global/ai-rewrite";
import { aiConnectionStore } from "../stores/global/ai-connection";
import { ON_DEVICE_PROVIDER } from "../stores/global/ai-providers";
import { configStore } from "../stores/global/config";
import { REWRITE_ACTIONS } from "./rewrite-actions";
import type { Command } from "../types/commands";
import type { AiEndpointState } from "../stores/global/ai-rewrite";
import type { AiAction } from "../services/tauri";

export type { AiAction };

/** What a base URL the guard turns away has to be instead. */
export const BASE_URL_RULE =
  "Use https, or http for this machine or an IP address on your local network.";

/** How the consent sheet for an address on a private network describes the
 * connection check. */
export const PRIVATE_CHECK =
  "Writ also checks the address is reachable. The check sends no text.";

/**
 * Resolves everything that would block a rewrite, in one pass, before any text
 * is sent.
 *
 * The endpoint is resolved in Rust by the same code the request guard uses, so
 * consent is always recorded under the exact host the guard later checks. The
 * key is asked for first, and then one sheet at most: consent to the host,
 * which also says the whole document goes when nothing is selected, or for a
 * host already allowed, the whole-document question alone (CH-05).
 *
 * `wholeDocument` is the character count of the document going in place of a
 * selection, or null when a selection goes.
 *
 * Returns `true` when the rewrite may proceed.
 */
async function clearBlockersBeforeSending(wholeDocument: number | null): Promise<boolean> {
  let endpoint: AiEndpointState;
  try {
    endpoint = await aiRewriteStore.endpointState();
  } catch {
    showToast("Could not read the connection settings.", "error");
    return false;
  }

  // Nothing leaves the Mac: no address, key or consent to clear.
  if (endpoint.is_on_device) return confirmWholeDocument(wholeDocument);

  if (!endpoint.is_allowed || !endpoint.host) {
    const open = await requestConfirm({
      title: "This base URL cannot be used",
      message: BASE_URL_RULE,
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.base_url");
    return false;
  }

  if (endpoint.is_hosted && !endpoint.is_private_network && !endpoint.key_state.is_set) {
    const open = await requestConfirm({
      title: `Add an API key for ${endpoint.host}`,
      message:
        "The key goes in your keychain, or in memory for this session when the keychain is unavailable. It is never written to config.toml.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.api_key");
    return false;
  }

  if (endpoint.is_hosted && !endpoint.is_consented) {
    const host = endpoint.host_port ?? endpoint.host;
    const confirmed = await requestConfirm({
      title: `Send text to ${host}?`,
      message: endpoint.is_private_network
        ? wholeDocument === null
          ? `The text you rewrite is sent to this address on your local network. ${PRIVATE_CHECK}`
          : `No text is selected, so the whole document (${wholeDocument} characters) is sent to this address on your local network. ${PRIVATE_CHECK}`
        : wholeDocument === null
          ? "The text you rewrite is sent with your API key. Writ also sends the key on its own to check the host is reachable. The check sends no text."
          : `No text is selected, so the whole document (${wholeDocument} characters) is sent with your API key. Writ also sends the key on its own to check the host is reachable. The check sends no text.`,
      confirmLabel: "Send",
      defaultAction: "cancel",
    });
    if (!confirmed) return false;
    try {
      await aiRewriteStore.consentHost();
    } catch {
      showToast("Could not record the choice.", "error");
      return false;
    }
    return true;
  }

  return confirmWholeDocument(wholeDocument);
}

/** Asks before the whole document goes in place of a selection. */
async function confirmWholeDocument(wholeDocument: number | null): Promise<boolean> {
  if (wholeDocument === null) return true;
  return requestConfirm({
    title: "Rewrite the whole document?",
    message: `No text is selected. Send the whole document (${wholeDocument} characters) to the model?`,
    confirmLabel: "Send",
  });
}

/** Runs a rewrite action from any entry point (palette command, status-bar
 * menu, or the editor context menu), acting on the active editor's selection.
 *
 * `presetRange` lets a caller supply a range captured earlier — the context
 * menu pins the selection when it opens, so an edit while the menu is up cannot
 * silently retarget the rewrite. */
export async function runRewriteAction(action: AiAction, presetRange?: AnchoredRange) {
  const ai = configStore.config().ai;
  const model = ai.model.trim();
  // The on-device model is the only one there is, so there is none to choose.
  if (!model && ai.provider !== ON_DEVICE_PROVIDER) {
    const open = await requestConfirm({
      title: "Choose a model",
      message: "No model is set.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.model");
    return;
  }
  // The last probe found the endpoint but not this model — say so instead of
  // failing mid-request.
  const status = aiConnectionStore.status();
  if (status?.kind === "model_missing" && status.detail === model) {
    showToast({
      text: `"${model}" is not available on the server.`,
      type: "info",
      durationMs: 5000,
      action: { label: "Connection settings", run: () => openSettings("apps", "ai.model") },
    });
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

  if (!(await clearBlockersBeforeSending(range.usedSelection ? null : range.text.length))) return;

  aiRewriteStore.start(action, range);
}

/** The rewrite commands, one per action. They enter the registry through
 * `defineAppCommands`, so they follow the Rewrite switch like every app's. */
export function rewriteCommands(): Command[] {
  return REWRITE_ACTIONS.map((action) => ({
    id: action.commandId,
    label: action.label,
    description: action.description,
    keywords: action.keywords,
    scope: "app",
    execute: () => void runRewriteAction(action.id),
  }));
}
