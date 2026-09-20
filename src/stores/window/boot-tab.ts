import type { BufferDocument } from "../../types/buffer";
import type { FirstRunStep } from "../global/first-run";

/** The tab actions the boot decision reaches, which is all it is given. */
export interface BootTabActions {
  activeTabId(): string | null;
  setActiveTabId(id: string | null): void;
  createTab(title?: string): Promise<unknown>;
}

/**
 * Puts the window on the tab it opens with: the note the last session left, a
 * new one when there is nothing to restore, and nothing at all while the first
 * launch is still asking its question.
 *
 * A window that already has an active tab is left alone: something earlier in
 * the boot has already answered this, and a second answer would move the
 * reader off the note they are looking at.
 *
 * It lives here rather than inside `App`'s `onMount` so the order it decides in
 * can be tested. The first launch mints nothing until Continue is pressed, and
 * the note that answer carries is the one that opens, so a tab minted behind
 * the screen would be a second empty note the person never asked for.
 */
export async function openBootTab(
  tabs: BootTabActions,
  activeTabs: readonly BufferDocument[],
  step: FirstRunStep | null,
): Promise<void> {
  if (tabs.activeTabId() !== null) return;
  const last = activeTabs[activeTabs.length - 1];
  if (last) {
    tabs.setActiveTabId(last.id);
    return;
  }
  if (step !== null) return;
  await tabs.createTab();
}
