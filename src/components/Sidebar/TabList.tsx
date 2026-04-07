import { For, createMemo } from "solid-js";
import TabItem from "./TabItem";
import { bufferStore } from "../../stores/buffers";
import { sidebarStore } from "../../stores/sidebar";
import { showContextMenu } from "../ContextMenu/ContextMenu";

export default function TabList() {
  const filtered = createMemo(() => {
    const query = sidebarStore.searchQuery().toLowerCase();
    const tabs = bufferStore.activeTabs();
    if (!query) return tabs;
    const matchedIds = sidebarStore.searchResultIds();
    if (matchedIds.length > 0) {
      return tabs.filter(b => b.title.toLowerCase().includes(query) || matchedIds.includes(b.id));
    }
    return tabs.filter(b => b.title.toLowerCase().includes(query));
  });

  function handleContextMenu(e: MouseEvent, tabId: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Close Tab", action: () => bufferStore.closeTab(tabId) },
      { label: "Close Other Tabs", action: () => bufferStore.closeOtherTabs(tabId) },
      { label: "Close All Tabs", action: () => bufferStore.closeAllTabs(), separator: true, danger: true },
    ]);
  }

  return (
    <div class="tab-list">
      <For each={filtered()} fallback={<div class="tab-list-empty">No active tabs</div>}>
        {(tab) => (
          <div onContextMenu={(e) => handleContextMenu(e, tab.id)}>
            <TabItem
              title={tab.title}
              isActive={bufferStore.activeTabId() === tab.id}
              onClick={() => bufferStore.setActiveTabId(tab.id)}
              onClose={() => bufferStore.closeTab(tab.id)}
            />
          </div>
        )}
      </For>
    </div>
  );
}
