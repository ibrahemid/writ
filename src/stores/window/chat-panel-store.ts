import { createSignal } from "solid-js";
import { configStore, clampChatWidth } from "../global/config";

export type ChatPanelStore = ReturnType<typeof createChatPanelStore>;

/**
 * The chat column: whether it is showing and how wide it is.
 *
 * Per-window state, like the sidebar's and the panel's, and what persists is
 * what a person set. Whether the column can be opened at all is a different
 * question, answered by `ai.chat.enabled`; this store answers only where the
 * column was left.
 */
export function createChatPanelStore() {
  const [isOpen, setIsOpen] = createSignal(false);

  function hydrateFromConfig() {
    setIsOpen(configStore.config().chat_panel.open);
  }

  /** The persisted width, clamped, so a hand-edited config cannot widen it. */
  function width(): number {
    return clampChatWidth(configStore.config().chat_panel.width);
  }

  function setWidth(next: number) {
    configStore.setChatPanelWidth(next);
  }

  function show() {
    setIsOpen(true);
    configStore.setChatPanelOpen(true);
  }

  function hide() {
    setIsOpen(false);
    configStore.setChatPanelOpen(false);
  }

  function toggle() {
    const next = !isOpen();
    setIsOpen(next);
    configStore.setChatPanelOpen(next);
  }

  return { isOpen, show, hide, toggle, hydrateFromConfig, width, setWidth };
}
