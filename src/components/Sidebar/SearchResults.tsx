import { For, Show, createMemo } from "solid-js";
import TabItem from "./TabItem";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { matchedBuffers } from "./search-results";

export default function SearchResults() {
  const win = useWindow();
  const matches = createMemo(() =>
    matchedBuffers(
      win.sidebar.searchQuery(),
      win.sidebar.searchResultIds(),
      bufferRegistry.activeTabs(),
      bufferRegistry.historyList(),
    ),
  );

  const total = createMemo(
    () => matches().active.length + matches().history.length,
  );

  function activeContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Close Tab", action: () => void win.tabs.closeTab(id) },
      { label: "Close Other Tabs", action: () => void win.tabs.closeOtherTabs(id) },
    ]);
  }

  function historyContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Restore", action: () => void win.tabs.restoreFromHistory(id) },
      { label: "Delete", action: () => void bufferRegistry.deleteFromHistory(id), danger: true },
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
                    isActive={win.tabs.activeTabId() === b.id}
                    onClick={() => win.tabs.setActiveTabId(b.id)}
                    onClose={() => void win.tabs.closeTab(b.id)}
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
                    onClick={() => void win.tabs.restoreFromHistory(b.id)}
                    onRestore={() => void win.tabs.restoreFromHistory(b.id)}
                    onClose={() => void bufferRegistry.deleteFromHistory(b.id)}
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
