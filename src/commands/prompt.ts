import { textTools } from "../stores/global/text-tools";
import { clipboardStore } from "../stores/global/clipboard";
import { windowRegistry } from "../stores/global/window-registry";
import { showToast } from "../stores/global/notifications";
import { contextPackStore } from "../stores/global/context-pack";
import { requestPlaceholderFill } from "../components/PromptFill/PromptFillModal";
import { registerCommand } from "./registry";
import { logFailure } from "../lib/log";

const PREPARE_PROMPT_TRANSFORM_ID = "prepare_prompt";

export function registerPromptCommands(): void {
  registerCommand({
    id: "prompt.copyAsPrompt",
    label: "Copy as prompt",
    description: "Strip frontmatter and comments, copy the result to the clipboard",
    scope: "app",
    execute: () => {
      void copyAsPrompt();
    },
  });

  registerCommand({
    id: "prompt.copyFiles",
    label: "Copy files as prompt…",
    description: "This file with the files it links to, a folder or a tag, as one prompt",
    keywords: ["context", "pack", "linked", "links", "folder", "tag", "tokens", "llm"],
    scope: "app",
    execute: () => {
      void contextPackStore.open();
    },
  });

  registerCommand({
    id: "prompt.fillPlaceholders",
    label: "Fill placeholders…",
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
    const stripped = await textTools.applyTransform(PREPARE_PROMPT_TRANSFORM_ID, read.text);
    await clipboardStore.copyText(stripped);
    showToast("Copied as prompt", "success");
  } catch {
    logFailure("copy as prompt failed");
    showToast("Could not copy as prompt", "error");
  }
}

async function fillPlaceholders(): Promise<void> {
  const read = windowRegistry.getActive()?.editor.getActiveText(true);
  if (!read) return;
  try {
    const names = await textTools.scanPlaceholders(read.text);
    const values = await requestPlaceholderFill(names, read.text);
    if (values === null) return;
    const filled = await textTools.fillPlaceholders(read.text, values);
    await clipboardStore.copyText(filled);
    showToast("Filled prompt copied", "success");
  } catch {
    logFailure("filling placeholders failed");
    showToast("Could not fill the placeholders", "error");
  }
}
