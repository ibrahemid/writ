import { requestConfirm } from "../components/ConfirmDialog/ConfirmDialog";
import { showToast } from "../components/Notifications/Toast";
import { openSettings } from "../components/SettingsModal/SettingsModal";
import { aiConnectionStore } from "../stores/global/ai-connection";
import {
  chatStore,
  isAbsolutePath,
  noteName,
  totalBytes,
  type Attachment,
} from "../stores/global/chat";
import { configStore } from "../stores/global/config";
import { windowRegistry } from "../stores/global/window-registry";
import type { ChatEndpointState } from "../services/tauri";

export const CHAT_TOGGLE_COMMAND_ID = "chat.toggle";

/** How the send dialog and the pane say a size. */
export function byteLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

/** How many files, as a chip and the send dialog both say it. */
export function noteCount(count: number): string {
  return count === 1 ? "1 file" : `${count} files`;
}

/** What the dialog says is being sent, and where.
 *
 * One file is named, because a person agreeing to send a file wants to know
 * which one. A set is counted. */
export function sendNotice(host: string, attachments: readonly Attachment[]) {
  const bytes = byteLabel(totalBytes(attachments));
  if (attachments.length === 1) {
    const name = noteName(attachments[0].key ?? attachments[0].path);
    return {
      title: `Send ${name} to ${host}?`,
      message: `${name} (${bytes}) and this message go to ${host} with your API key.`,
    };
  }
  return {
    title: `Send files to ${host}?`,
    message: `${attachments.length} files (${bytes}) and this message go to ${host} with your API key.`,
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
      message: "Turn it on in Settings, Apps.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.chat.enabled");
    return false;
  }

  if (!endpoint.is_allowed || !endpoint.host) {
    const open = await requestConfirm({
      title: "This base URL cannot be used",
      message: "Use https, or http only for a server on this machine.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.provider");
    return false;
  }

  if (!endpoint.model.trim()) {
    const open = await requestConfirm({
      title: "Choose a model",
      message: "No model is set.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.model");
    return false;
  }

  // A live catalog is the provider's own inventory, so a model missing from
  // one cannot answer. A curated list is a table of suggestions and proves
  // nothing, so it does not stop a send.
  const catalog = aiConnectionStore.catalog();
  if (catalog?.source === "live" && !catalog.models.includes(endpoint.model)) {
    const open = await requestConfirm({
      title: `${endpoint.model} is not available`,
      message: `${endpoint.provider} does not list it. Choose one it has.`,
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.model");
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
      await aiConnectionStore.consentHost();
      endpoint = await chatStore.endpointState();
    } catch {
      showToast("Could not record the choice.", "error");
      return false;
    }
  }

  if (endpoint.is_hosted && !endpoint.key_state.is_set) {
    const open = await requestConfirm({
      title: `Add an API key for ${endpoint.host}`,
      message:
        "The key goes in your keychain, or in memory for this session when the keychain is unavailable. It is never written to config.toml.",
      confirmLabel: "Open settings",
    });
    if (open) openSettings("apps", "ai.api_key");
    return false;
  }

  return true;
}

/** Sends the draft after the blockers are cleared.
 *
 * The dialog is shown the sizes the files hold now, not the sizes the tabs
 * recorded when they read them, so the number a person agrees to is the number
 * that leaves the machine. The list is read once and handed to the send, so
 * the notes counted in the dialog are the notes the request carries. */
export async function sendChatMessage() {
  if (!chatStore.draft().trim()) return;
  const attachments = await chatStore.attachedOnDisk();
  chatStore.setAttachedList(attachments);
  const unreadable = attachments.find((note) => note.state === "unreadable");
  if (unreadable) {
    showToast(unreadable.reason ?? `${unreadable.name} could not be read.`, "error");
    return;
  }
  if (!(await clearBlockersBeforeSending(attachments))) return;
  await chatStore.send(attachments);
}

/** Shows the pane, or hides it when it is already showing. */
export function toggleChatPane() {
  windowRegistry.getActive()?.chatPanel.toggle();
}

/** Shows the pane, or opens the switch that would make one.
 *
 * The command stays registered while chat is off, unavailable, so the palette
 * and both menus leave it out while its chord still answers: with no pane to
 * show, the chord lands on the Chat row in Settings, Apps (ADR-042 section 3). */
export function toggleChat() {
  if (configStore.isAppOn("chat")) {
    toggleChatPane();
    return;
  }
  openSettings("apps", "ai.chat.enabled");
}
