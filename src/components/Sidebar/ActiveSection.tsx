import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import TabItem from "./TabItem";
import Icon from "../Icon/Icon";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { groupActiveByDirectory, SCRATCH_GROUP_KEY } from "./grouping";
import type { BufferDocument } from "../../types/buffer";
import "./ActiveSection.css";

export default function ActiveSection() {
  const win = useWindow();
  const [collapsed, setCollapsed] = createSignal<Set<string>>(new Set());

  const groups = createMemo(() =>
    groupActiveByDirectory(bufferRegistry.activeTabs(), win.tabs.activeTabId()),
  );

  const total = createMemo(() =>
    groups().reduce((count, group) => count + group.items.length, 0),
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

  function row(item: BufferDocument): JSX.Element {
    return (
      <div class="active-row" onContextMenu={(e) => handleContextMenu(e, item.id)}>
        <TabItem
          label={item.title}
          icon="file-text"
          isActive={item.id === win.tabs.activeTabId()}
          onClick={() => win.tabs.setActiveTabId(item.id)}
          onClose={() => void win.tabs.closeTab(item.id)}
        />
      </div>
    );
  }

  return (
    <Show when={groups().length > 0}>
      <div class="sidebar-section active-section">
        <div class="sidebar-section-title">
          Open
          <span class="sidebar-section-count">{total()}</span>
        </div>
        <div class="active-list">
          <For each={groups()}>
            {(group) => (
              <Show
                when={group.key !== SCRATCH_GROUP_KEY}
                fallback={
                  // Notes with no file behind them yet need no heading: they
                  // are what "Open" already says they are.
                  <div class="active-group">
                    <For each={group.items}>{(item) => row(item)}</For>
                  </div>
                }
              >
                <div class="active-group">
                  <button
                    type="button"
                    class="active-group-head"
                    classList={{ "is-collapsed": collapsed().has(group.key) }}
                    onClick={() => toggleGroup(group.key)}
                  >
                    <span class="active-group-caret" aria-hidden="true">
                      <Icon name="caret-down" size={12} />
                    </span>
                    <span class="active-group-name">{group.label}</span>
                    <span class="active-group-count">{group.items.length}</span>
                  </button>
                  <Show when={!collapsed().has(group.key)}>
                    <div class="active-group-items">
                      <For each={group.items}>{(item) => row(item)}</For>
                    </div>
                  </Show>
                </div>
              </Show>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
}
