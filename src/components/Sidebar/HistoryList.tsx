import { For, createMemo } from "solid-js";
import TabItem from "./TabItem";
import { bufferStore } from "../../stores/buffers";
import { sidebarStore } from "../../stores/sidebar";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import type { BufferDocument } from "../../types/buffer";

const DAY_MS = 86_400_000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

function groupLabel(closedAt: string | null, today: number): string {
  if (!closedAt) return "Earlier";
  const ts = new Date(closedAt).getTime();
  if (Number.isNaN(ts)) return "Earlier";
  const day = startOfDay(ts);
  if (day === today) return "Today";
  if (day === today - DAY_MS) return "Yesterday";
  if (day > today - 7 * DAY_MS) return "This Week";
  return new Date(ts).toLocaleDateString();
}

export default function HistoryList() {
  const filtered = createMemo(() => {
    const query = sidebarStore.searchQuery().toLowerCase();
    const history = bufferStore.historyList();
    if (!query) return history;
    const matchedIds = sidebarStore.searchResultIds();
    if (matchedIds.length > 0) {
      return history.filter(b => b.title.toLowerCase().includes(query) || matchedIds.includes(b.id));
    }
    return history.filter(b => b.title.toLowerCase().includes(query));
  });

  const groups = createMemo(() => {
    const today = startOfDay(Date.now());
    const sorted = [...filtered()].sort((a, b) => {
      const ta = a.closed_at ? new Date(a.closed_at).getTime() : 0;
      const tb = b.closed_at ? new Date(b.closed_at).getTime() : 0;
      return tb - ta;
    });
    const buckets = new Map<string, BufferDocument[]>();
    for (const item of sorted) {
      const label = groupLabel(item.closed_at, today);
      const bucket = buckets.get(label);
      if (bucket) {
        bucket.push(item);
      } else {
        buckets.set(label, [item]);
      }
    }
    return Array.from(buckets, ([label, items]) => ({ label, items }));
  });

  function handleContextMenu(e: MouseEvent, itemId: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Restore", action: () => bufferStore.restoreFromHistory(itemId) },
      { label: "Delete", action: () => bufferStore.deleteFromHistory(itemId), danger: true },
      { label: "Clear All History", action: () => bufferStore.clearAllHistory(), separator: true, danger: true },
    ]);
  }

  return (
    <div class="history-list">
      <For each={groups()} fallback={<div class="tab-list-empty">No history</div>}>
        {(group) => (
          <div class="history-group">
            <div class="history-group-title">{group.label}</div>
            <For each={group.items}>
              {(item) => (
                <div onContextMenu={(e) => handleContextMenu(e, item.id)}>
                  <TabItem
                    title={item.title}
                    onClick={() => bufferStore.restoreFromHistory(item.id)}
                    onRestore={() => bufferStore.restoreFromHistory(item.id)}
                    onClose={() => bufferStore.deleteFromHistory(item.id)}
                  />
                </div>
              )}
            </For>
          </div>
        )}
      </For>
    </div>
  );
}
