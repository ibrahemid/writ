import { configStore, clampChatWidth } from "../global/config";

export type ChatPanelStore = ReturnType<typeof createChatPanelStore>;

/**
 * The chat column: whether it is showing and how wide it is.
 *
 * Both are read from `configStore` rather than copied out of it, like the
 * sidebar's and the panel's. Whether the column can be opened at all is a
 * different question, answered by `ai.chat.enabled`; this store answers only
 * where the column was left.
 */
export function createChatPanelStore() {
  /** Closed until the stored settings have loaded. */
  const isOpen = (): boolean => configStore.loaded() && configStore.config().chat_panel.open;

  /** The persisted width, clamped, so a hand-edited config cannot widen it. */
  function width(): number {
    return clampChatWidth(configStore.config().chat_panel.width);
  }

  function setWidth(next: number) {
    configStore.setChatPanelWidth(next);
  }

  function show() {
    configStore.setChatPanelOpen(true);
  }

  function hide() {
    configStore.setChatPanelOpen(false);
  }

  function toggle() {
    configStore.setChatPanelOpen(!isOpen());
  }

  return { isOpen, show, hide, toggle, width, setWidth };
}
