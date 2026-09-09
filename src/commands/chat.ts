import { registerCommand, unregisterCommand } from "./registry";
import { requestConfirm } from "../components/ConfirmDialog/ConfirmDialog";
import { showToast } from "../components/Notifications/Toast";
import { openSettings } from "../components/SettingsModal/SettingsModal";
import { chatStore, totalBytes, type Attachment } from "../stores/global/chat";
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
      message: "Turn it on in AI settings to use this pane.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.chat_enabled");
    return false;
  }

  if (!endpoint.is_allowed || !endpoint.host) {
    const open = await requestConfirm({
      title: "This base URL cannot be used",
      message: "Use https, or http only for a server on this machine.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.chat_base_url");
    return false;
  }

  if (!endpoint.model.trim()) {
    const open = await requestConfirm({
      title: "Choose a chat model",
      message: "No model is set.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("ai", "ai.chat_model");
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
      await aiConsentHost("chat");
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
    if (open) openSettings("ai", "ai.chat_api_key");
    return false;
  }

  return true;
}

/** Sends the draft after the blockers are cleared. */
export async function sendChatMessage() {
  if (!chatStore.draft().trim()) return;
  if (!(await clearBlockersBeforeSending(chatStore.attachments()))) return;
  await chatStore.send();
}

/** Shows the pane, or hides it when it is already showing. */
export function toggleChatPane() {
  windowRegistry.getActive()?.chatPanel.toggle();
}

let registered = false;

export function registerChatCommands() {
  if (registered) return;
  registered = true;
  registerCommand({
    id: CHAT_TOGGLE_COMMAND_ID,
    icon: "chat-text",
    label: "Chat",
    description: "Ask a model about the notes you attach",
    keywords: ["chat", "ai", "ask", "model"],
    scope: "app",
    execute: () => toggleChatPane(),
  });
}

export function unregisterChatCommands() {
  if (!registered) return;
  registered = false;
  unregisterCommand(CHAT_TOGGLE_COMMAND_ID);
}
