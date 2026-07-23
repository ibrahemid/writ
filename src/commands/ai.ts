import { registerCommand, unregisterCommand } from "./registry";
import { windowRegistry } from "../stores/global/window-registry";
import { requestConfirm } from "../components/ConfirmDialog/ConfirmDialog";
import { showToast } from "../components/Notifications/Toast";
import { aiRewriteStore } from "../stores/global/ai-rewrite";
import type { AiAction } from "../services/tauri";

export type { AiAction };

const AI_COMMAND_IDS = ["ai.proofread", "ai.rephrase", "ai.polish", "ai.custom"] as const;

/** Runs a rewrite action from any entry point (palette command or status-bar
 * menu), acting on the active editor's selection. */
export async function runRewriteAction(action: AiAction) {
  const win = windowRegistry.getActive();
  if (!win) return;
  const bufferId = win.editor.currentBufferId();
  if (!bufferId) return;

  const range = win.editor.getSelectionRange(true);
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

  aiRewriteStore.start(action, { ...range, bufferId });
}

let registered = false;

export function registerAiCommands() {
  if (registered) return;
  registered = true;

  registerCommand({
    id: "ai.proofread",
    label: "Proofread selection",
    description: "Fix spelling, grammar, and punctuation with the configured model",
    scope: "app",
    execute: () => void runRewriteAction("proofread"),
  });
  registerCommand({
    id: "ai.rephrase",
    label: "Rephrase selection",
    description: "Restate the same meaning in different wording",
    scope: "app",
    execute: () => void runRewriteAction("rephrase"),
  });
  registerCommand({
    id: "ai.polish",
    label: "Polish selection",
    description: "Tighten and smooth while keeping the meaning and voice",
    scope: "app",
    execute: () => void runRewriteAction("polish"),
  });
  registerCommand({
    id: "ai.custom",
    label: "Custom rewrite…",
    description: "Rewrite the selection with your own instruction",
    scope: "app",
    execute: () => void runRewriteAction("custom"),
  });
}

export function unregisterAiCommands() {
  if (!registered) return;
  registered = false;
  for (const id of AI_COMMAND_IDS) unregisterCommand(id);
}
