import { For, Show, createMemo, createSignal } from "solid-js";
import TabItem from "./TabItem";
import { bufferStore } from "../../stores/buffers";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { groupActiveByDirectory } from "./grouping";
import "./ActiveSection.css";

export default function ActiveSection() {
  const [collapsed, setCollapsed] = createSignal<Set<string>>(new Set());

  const groups = createMemo(() =>
    groupActiveByDirectory(bufferStore.activeTabs(), bufferStore.activeTabId()),
  );

  function toggleGroup(key: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }

  function handleContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Close", action: () => bufferStore.closeTab(id) },
      {
        label: "Close Others",
        action: () => bufferStore.closeOtherTabs(id),
      },
    ]);
  }

  return (
    <Show when={groups().length > 0}>
      <div class="sidebar-section active-section">
        <div class="sidebar-section-title">Active</div>
        <div class="active-list">
          <For each={groups()}>
            {(group) => (
              <div class="active-group">
                <button
                  type="button"
                  class="active-group-head"
                  classList={{ "is-collapsed": collapsed().has(group.key) }}
                  onClick={() => toggleGroup(group.key)}
                >
                  <span class="active-group-chevron" aria-hidden="true">
                    ▾
                  </span>
                  <span class="active-group-name">{group.label}</span>
                  <span class="active-group-count">{group.items.length}</span>
                </button>
                <Show when={!collapsed().has(group.key)}>
                  <div class="active-group-items">
                    <For each={group.items}>
                      {(item) => (
                        <div
                          class="active-row"
                          onContextMenu={(e) => handleContextMenu(e, item.id)}
                        >
                          <TabItem
                            title={item.title}
                            isActive={item.id === bufferStore.activeTabId()}
                            onClick={() => bufferStore.setActiveTabId(item.id)}
                            onClose={() => bufferStore.closeTab(item.id)}
                          />
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
}
