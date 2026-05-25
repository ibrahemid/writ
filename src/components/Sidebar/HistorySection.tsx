import { For, Show, createMemo } from "solid-js";
import TabItem from "./TabItem";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { bucketHistoryByTime, relativeTime } from "./grouping";
import "./HistorySection.css";

export default function HistorySection() {
  const win = useWindow();
  const buckets = createMemo(() =>
    bucketHistoryByTime(bufferRegistry.historyList(), Date.now()),
  );

  function handleContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Restore", action: () => void win.tabs.restoreFromHistory(id) },
      {
        label: "Delete",
        action: () => void bufferRegistry.deleteFromHistory(id),
        danger: true,
      },
      {
        label: "Clear All History",
        action: () => void bufferRegistry.clearAllHistory(),
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
                        onClick={() => void win.tabs.restoreFromHistory(item.id)}
                        onRestore={() => void win.tabs.restoreFromHistory(item.id)}
                        onClose={() => void bufferRegistry.deleteFromHistory(item.id)}
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
