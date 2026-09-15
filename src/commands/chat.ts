import { requestConfirm } from "../components/ConfirmDialog/ConfirmDialog";
import { showToast } from "../components/Notifications/Toast";
import { openSettings } from "../components/SettingsModal/SettingsModal";
import { chatStore, totalBytes, type Attachment } from "../stores/global/chat";
import { configStore } from "../stores/global/config";
import { windowRegistry } from "../stores/global/window-registry";
import type { ChatEndpointState } from "../services/tauri";
import { aiConsentHost } from "../services/tauri";

export const CHAT_TOGGLE_COMMAND_ID = "chat.toggle";

/** How the send dialog and the pane say a size. */
export function byteLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

/** What the dialog says is being sent, and where. */
export function sendNotice(host: string, attachments: readonly Attachment[]) {
  const count = attachments.length;
  const notes = count === 1 ? "1 note" : `${count} notes`;
  return {
    title: `Send notes to ${host}?`,
    message: `${notes} (${byteLabel(totalBytes(attachments))}) and this message go to ${host} with your API key.`,
  };
}

/**
 * Resolves everything that would block a send, in one pass, before any note
 * leaves the machine.
 *
 * The endpoint is resolved in Rust by the same code the send guard uses, so
 * consent is recorded under the exact host the guard later checks. Consent and
 * key are handled together, so a person who has neither is not stopped twice.
 *
 * Returns `true` when the send may proceed.
 */
export async function clearBlockersBeforeSending(
  attachments: readonly Attachment[],
): Promise<boolean> {
  let endpoint: ChatEndpointState;
  try {
    endpoint = await chatStore.endpointState();
  } catch {
    showToast("Could not read the AI settings.", "error");
    return false;
  }

  if (!endpoint.enabled) {
    const open = await requestConfirm({
      title: "Chat is turned off",
      message: "Turn it on in AI settings.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.chat.enabled");
    return false;
  }

  if (!endpoint.is_allowed || !endpoint.host) {
    const open = await requestConfirm({
      title: "This base URL cannot be used",
      message: "Use https, or http only for a server on this machine.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.provider");
    return false;
  }

  if (!endpoint.model.trim()) {
    const open = await requestConfirm({
      title: "Choose a model",
      message: "No model is set.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.model");
    return false;
  }

  if (endpoint.is_hosted && !endpoint.is_consented) {
    const host = endpoint.host_port ?? endpoint.host;
    const confirmed = await requestConfirm({
      ...sendNotice(host, attachments),
      confirmLabel: "Send",
    });
    if (!confirmed) return false;
    try {
      await aiConsentHost();
      endpoint = await chatStore.endpointState();
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

/** Sends the draft after the blockers are cleared.
 *
 * The dialog is shown the sizes the files hold now, not the sizes the tabs
 * recorded when they read them, so the number a person agrees to is the number
 * that leaves the machine. */
export async function sendChatMessage() {
  if (!chatStore.draft().trim()) return;
  let attachments = chatStore.attachments();
  try {
    attachments = await chatStore.attachedOnDisk();
  } catch {
    showToast("Could not read the attached notes.", "error");
    return;
  }
  if (!(await clearBlockersBeforeSending(attachments))) return;
  await chatStore.send();
}

/** Shows the pane, or hides it when it is already showing. */
export function toggleChatPane() {
  windowRegistry.getActive()?.chatPanel.toggle();
}

/** Shows the pane, or says where to turn it on.
 *
 * The command is registered whether chat is on or not, so the shortcut editor
 * lists its chord and the View menu item routes somewhere. With chat off there
 * is no pane to show, and the one thing to offer is the setting that would
 * make one. */
export async function toggleChat() {
  if (configStore.config().ai.chat.enabled) {
    toggleChatPane();
    return;
  }
  const open = await requestConfirm({
    title: "Chat is turned off",
    message: "Turn it on in AI settings.",
    confirmLabel: "Open settings",
  });
  if (open) openSettings("ai", "ai.chat.enabled");
}
