// Singleton state — Writ is single-window.
import * as tauri from "../../services/tauri";
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

export const linkStore = {
  openExternal,
  classify,
};
