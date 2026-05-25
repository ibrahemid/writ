import { For, Show, createMemo, createSignal } from "solid-js";
import TabItem from "./TabItem";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { groupActiveByDirectory } from "./grouping";
import "./ActiveSection.css";

export default function ActiveSection() {
  const win = useWindow();
  const [collapsed, setCollapsed] = createSignal<Set<string>>(new Set());

  const groups = createMemo(() =>
    groupActiveByDirectory(bufferRegistry.activeTabs(), win.tabs.activeTabId()),
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
      { label: "Close", action: () => void win.tabs.closeTab(id) },
      {
        label: "Close Others",
        action: () => void win.tabs.closeOtherTabs(id),
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
                            isActive={item.id === win.tabs.activeTabId()}
                            onClick={() => win.tabs.setActiveTabId(item.id)}
                            onClose={() => void win.tabs.closeTab(item.id)}
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
