import {
  applyTransform,
  promptScanPlaceholders,
  promptFillPlaceholders,
} from "../services/tauri";
import { writeClipboardText } from "../services/clipboard";
import { windowRegistry } from "../stores/global/window-registry";
import { showToast } from "../components/Notifications/Toast";
import { requestPlaceholderFill } from "../components/PromptFill/PromptFillModal";
import { registerCommand } from "./registry";

const PREPARE_PROMPT_TRANSFORM_ID = "prepare_prompt";

export function registerPromptCommands(): void {
  registerCommand({
    id: "prompt.copyAsPrompt",
    label: "Copy as Prompt",
    description: "Strip frontmatter and comments, copy the result to the clipboard",
    scope: "app",
    execute: () => {
      void copyAsPrompt();
    },
  });

  registerCommand({
    id: "prompt.fillPlaceholders",
    label: "Fill Placeholders…",
    description: "Fill {{placeholders}} and copy the result to the clipboard",
    scope: "app",
    execute: () => {
      void fillPlaceholders();
    },
  });
}

async function copyAsPrompt(): Promise<void> {
  const read = windowRegistry.getActive()?.editor.getActiveText(true);
  if (!read) return;
  try {
    const stripped = await applyTransform(PREPARE_PROMPT_TRANSFORM_ID, read.text);
    await writeClipboardText(stripped);
    showToast("Copied as prompt", "success");
  } catch (error) {
    console.error("prompt.copyAsPrompt failed", error);
    showToast("Copy as prompt failed", "error");
  }
}

async function fillPlaceholders(): Promise<void> {
  const read = windowRegistry.getActive()?.editor.getActiveText(true);
  if (!read) return;
  try {
    const names = await promptScanPlaceholders(read.text);
    const values = await requestPlaceholderFill(names);
    if (values === null) return;
    const filled = await promptFillPlaceholders(read.text, values);
    await writeClipboardText(filled);
    showToast("Filled prompt copied", "success");
  } catch (error) {
    console.error("prompt.fillPlaceholders failed", error);
    showToast("Fill placeholders failed", "error");
  }
}
