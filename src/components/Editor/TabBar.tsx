import { For, createSignal, createEffect, onCleanup, Show } from "solid-js";
import { bufferStore } from "../../stores/buffers";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { abbreviateTitle } from "../../lib/buffer-name";
import "./TabBar.css";

// Singleton state — Writ is single-window, single-instance per component
const [editingTabId, setEditingTabId] = createSignal<string | null>(null);

export function startRenameActiveTab() {
  const id = bufferStore.activeTabId();
  if (id) setEditingTabId(id);
}

export default function TabBar() {
  const tabEls = new Map<string, HTMLButtonElement>();

  createEffect(() => {
    const id = bufferStore.activeTabId();
    if (!id) return;
    const el = tabEls.get(id);
    if (!el) return;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollIntoView({
      behavior: reduceMotion ? "auto" : "smooth",
      inline: "nearest",
      block: "nearest",
    });
  });

  function handleRenameSubmit(tabId: string, value: string) {
    const trimmed = value.trim();
    if (trimmed) {
      bufferStore.renameTab(tabId, trimmed);
    }
    setEditingTabId(null);
  }

  function handleRenameKeyDown(e: KeyboardEvent, tabId: string) {
    if (e.key === "Enter") {
      handleRenameSubmit(tabId, (e.target as HTMLInputElement).value);
    } else if (e.key === "Escape") {
      setEditingTabId(null);
    }
  }

  function handleContextMenu(e: MouseEvent, tabId: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Rename", action: () => setEditingTabId(tabId) },
      { label: "Close Tab", action: () => bufferStore.closeTab(tabId) },
      { label: "Close Other Tabs", action: () => bufferStore.closeOtherTabs(tabId) },
      { label: "Close All Tabs", action: () => bufferStore.closeAllTabs(), separator: true, danger: true },
    ]);
  }

  return (
    <div class="tabbar">
      <div class="tabbar-tabs">
        <For each={bufferStore.activeTabs()}>
          {(tab) => (
            <button
              ref={(el) => {
                tabEls.set(tab.id, el);
                onCleanup(() => tabEls.delete(tab.id));
              }}
              class={`tab ${bufferStore.activeTabId() === tab.id ? "tab-active" : ""}`}
              onClick={() => bufferStore.setActiveTabId(tab.id)}
              onDblClick={() => setEditingTabId(tab.id)}
              onContextMenu={(e) => handleContextMenu(e, tab.id)}
              title={tab.title}
            >
              <Show when={editingTabId() === tab.id} fallback={
                <span class="tab-title">{abbreviateTitle(tab.title)}</span>
              }>
                <input
                  ref={(el) => {
                    requestAnimationFrame(() => {
                      el.focus();
                      el.select();
                    });
                  }}
                  class="tab-rename-input"
                  value={tab.title}
                  onBlur={(e) => handleRenameSubmit(tab.id, e.currentTarget.value)}
                  onKeyDown={(e) => handleRenameKeyDown(e, tab.id)}
                  onClick={(e) => e.stopPropagation()}
                />
              </Show>
              <span
                class="tab-close"
                role="button"
                tabIndex={0}
                onClick={(e) => { e.stopPropagation(); bufferStore.closeTab(tab.id); }}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.stopPropagation(); bufferStore.closeTab(tab.id); } }}
              >
                ×
              </span>
            </button>
          )}
        </For>
      </div>
      <button
        type="button"
        class="tabbar-new"
        aria-label="New tab"
        title="New tab"
        onClick={() => bufferStore.createTab()}
      >+</button>
    </div>
  );
}
