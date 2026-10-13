import { createRoot, createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import { unreadTabFor, type UnreadTab } from "../../lib/unread-tab";

/**
 * The tabs restored at launch whose files Writ has not read, by row id.
 *
 * Set once, from the rows the launch listed: every tab opened after that came
 * through an open that measured its file. A tab leaves the set when its open
 * answers with its row, or when it closes.
 */
function createUnreadTabs() {
  // Singleton state — Writ is single-window
  const [unread, setUnread] = createSignal<ReadonlyMap<string, UnreadTab>>(new Map());

  function stateOf(id: string): UnreadTab | null {
    return unread().get(id) ?? null;
  }

  /** Takes each tab the launch restored whose file has to wait. */
  function takeRestored(docs: readonly BufferDocument[]): void {
    setUnread((prev) => {
      const next = new Map(prev);
      for (const doc of docs) {
        const tab = unreadTabFor(doc);
        if (tab) next.set(doc.id, tab);
      }
      return next;
    });
  }

  // An unread tab's state moves with its open's answers. A tab no longer in
  // the set (its open answered, or it closed while the question was up) stays
  // out of it.
  function update(id: string, tab: UnreadTab): void {
    setUnread((prev) => (prev.has(id) ? new Map(prev).set(id, tab) : prev));
  }

  /** The unread tab's file is over the size Writ opens without asking. */
  function wait(id: string, sizeBytes: number): void {
    update(id, { kind: "waiting", sizeBytes });
  }

  /** The unread tab's file is not opened, for `reason`. */
  function refuse(id: string, reason: string): void {
    update(id, { kind: "refused", reason });
  }

  /** The tab's file opened, or the tab closed. */
  function clear(id: string): void {
    setUnread((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Map(prev);
      next.delete(id);
      return next;
    });
  }

  return { stateOf, takeRestored, wait, refuse, clear };
}

export const unreadTabs = createRoot(createUnreadTabs);
