import { For, Show, createMemo } from "solid-js";
import TabItem from "./TabItem";
import { bufferStore } from "../../stores/buffers";
import { sidebarStore } from "../../stores/sidebar";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { matchedBuffers } from "./search-results";

export default function SearchResults() {
  const matches = createMemo(() =>
    matchedBuffers(
      sidebarStore.searchQuery(),
      sidebarStore.searchResultIds(),
      bufferStore.activeTabs(),
      bufferStore.historyList(),
    ),
  );

  const total = createMemo(
    () => matches().active.length + matches().history.length,
  );

  function activeContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Close Tab", action: () => bufferStore.closeTab(id) },
      { label: "Close Other Tabs", action: () => bufferStore.closeOtherTabs(id) },
    ]);
  }

  function historyContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Restore", action: () => bufferStore.restoreFromHistory(id) },
      { label: "Delete", action: () => bufferStore.deleteFromHistory(id), danger: true },
    ]);
  }

  return (
    <div class="history-list">
      <Show
        when={total() > 0}
        fallback={<div class="tab-list-empty">No matches</div>}
      >
        <Show when={matches().active.length > 0}>
          <div class="history-group">
            <div class="history-group-title">Open</div>
            <For each={matches().active}>
              {(b) => (
                <div onContextMenu={(e) => activeContextMenu(e, b.id)}>
                  <TabItem
                    title={b.title}
                    isActive={bufferStore.activeTabId() === b.id}
                    onClick={() => bufferStore.setActiveTabId(b.id)}
                    onClose={() => bufferStore.closeTab(b.id)}
                  />
                </div>
              )}
            </For>
          </div>
        </Show>
        <Show when={matches().history.length > 0}>
          <div class="history-group">
            <div class="history-group-title">History</div>
            <For each={matches().history}>
              {(b) => (
                <div onContextMenu={(e) => historyContextMenu(e, b.id)}>
                  <TabItem
                    title={b.title}
                    onClick={() => bufferStore.restoreFromHistory(b.id)}
                    onRestore={() => bufferStore.restoreFromHistory(b.id)}
                    onClose={() => bufferStore.deleteFromHistory(b.id)}
                  />
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </div>
  );
}
