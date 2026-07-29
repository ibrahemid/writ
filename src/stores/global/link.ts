// Singleton state — Writ is single-window.
import * as tauri from "../../services/tauri";
import { writeClipboardText } from "../../services/clipboard";
import { showToast } from "../../components/Notifications/Toast";
import type { LinkVerdict } from "../../types/link";

const FALLBACK_MESSAGE = "Could not open the link.";

// A refused link surfaces the reason Rust gave, so a `javascript:` or `file:`
// destination reads as a rule rather than as a silent failure.
async function openExternal(url: string): Promise<void> {
  try {
    await tauri.openExternalUrl(url);
  } catch (err) {
    showToast(typeof err === "string" && err !== "" ? err : FALLBACK_MESSAGE, "error");
  }
}

async function classify(url: string): Promise<LinkVerdict> {
  return tauri.classifyExternalUrl(url);
}

/** Copies a link destination, reporting failure rather than swallowing it. */
async function copyLink(text: string): Promise<void> {
  try {
    await writeClipboardText(text);
  } catch {
    showToast("Could not copy the link.", "error");
  }
}

export const linkStore = {
  openExternal,
  classify,
  copyLink,
};
