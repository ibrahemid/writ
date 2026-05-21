import { For, Show, createMemo } from "solid-js";
import TabItem from "./TabItem";
import { bufferStore } from "../../stores/buffers";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { bucketHistoryByTime, relativeTime } from "./grouping";
import "./HistorySection.css";

export default function HistorySection() {
  const buckets = createMemo(() =>
    bucketHistoryByTime(bufferStore.historyList(), Date.now()),
  );

  function handleContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Restore", action: () => bufferStore.restoreFromHistory(id) },
      {
        label: "Delete",
        action: () => bufferStore.deleteFromHistory(id),
        danger: true,
      },
      {
        label: "Clear All History",
        action: () => bufferStore.clearAllHistory(),
        separator: true,
        danger: true,
      },
    ]);
  }

  return (
    <Show when={buckets().length > 0}>
      <div class="sidebar-section history-section">
        <div class="sidebar-section-title">History</div>
        <div class="history-list">
          <For each={buckets()}>
            {(bucket) => (
              <div class="history-group">
                <div class="history-group-title">{bucket.label}</div>
                <For each={bucket.items}>
                  {(item) => (
                    <div onContextMenu={(e) => handleContextMenu(e, item.id)}>
                      <TabItem
                        title={item.title}
                        trailing={relativeTime(
                          item.closed_at ?? item.updated_at,
                          Date.now(),
                        )}
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
      </div>
    </Show>
  );
}
