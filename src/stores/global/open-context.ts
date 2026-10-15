import { answerOpenContext } from "../../services/tauri";
import { logFailure } from "../../lib/log";
import { windowContext } from "../../lib/open-context";
import { windowRegistry } from "./window-registry";

/**
 * Answers a program allowed to see the open file (ADR-053): the tab in front
 * in this window and what is selected in it. Rust checked the program's grant
 * before it asked, names the file from the tab, and writes the answer the
 * program reads.
 */
export async function answerContextAsk(token: string): Promise<void> {
  const win = windowRegistry.getActive();
  const id = win?.editor.currentBufferId() ?? null;
  const state = win?.editor.getView()?.state ?? null;
  const unsaved = win !== null && id !== null && win.editor.isDirty(id);
  try {
    await answerOpenContext(token, windowContext(id, state, unsaved));
  } catch {
    logFailure("a program's question about the open file could not be answered");
  }
}
