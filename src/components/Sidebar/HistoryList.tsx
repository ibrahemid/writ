import { For, createMemo } from "solid-js";
import TabItem from "./TabItem";
import { bufferStore } from "../../stores/buffers";
import { sidebarStore } from "../../stores/sidebar";
import { showContextMenu } from "../ContextMenu/ContextMenu";

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
      <For each={filtered()} fallback={<div class="tab-list-empty">No history</div>}>
        {(item) => (
          <div onContextMenu={(e) => handleContextMenu(e, item.id)}>
            <TabItem
              title={item.title}
              onClick={() => bufferStore.restoreFromHistory(item.id)}
              onRestore={() => bufferStore.restoreFromHistory(item.id)}
              onClose={() => bufferStore.deleteFromHistory(item.id)}
              secondary={item.closed_at ? new Date(item.closed_at).toLocaleDateString() : undefined}
            />
          </div>
        )}
      </For>
    </div>
  );
}
