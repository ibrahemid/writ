import { For, createSignal, createEffect, onCleanup, Show } from "solid-js";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { windowRegistry } from "../../stores/global/window-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { abbreviateTitle } from "../../lib/buffer-name";
import "./TabBar.css";

// Module-level singleton — TabBar mounts only in the main window (detached
// preview windows render no tabstrip per ADR-009). Editing state is the
// single TabBar instance's local UI state, not per-window logical state.
const [editingTabId, setEditingTabId] = createSignal<string | null>(null);

export function startRenameActiveTab() {
  const w = windowRegistry.getActive();
  if (!w) return;
  const id = w.tabs.activeTabId();
  if (id) setEditingTabId(id);
}

export default function TabBar() {
  const win = useWindow();
  const tabEls = new Map<string, HTMLButtonElement>();

  createEffect(() => {
    const id = win.tabs.activeTabId();
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
      void bufferRegistry.renameBuffer(tabId, trimmed);
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
      { label: "Close Tab", action: () => void win.tabs.closeTab(tabId) },
      { label: "Close Other Tabs", action: () => void win.tabs.closeOtherTabs(tabId) },
      { label: "Close All Tabs", action: () => void win.tabs.closeAllTabs(), separator: true, danger: true },
    ]);
  }

  return (
    <div class="tabbar">
      <div class="tabbar-tabs">
        <For each={bufferRegistry.activeTabs()}>
          {(tab) => (
            <button
              ref={(el) => {
                tabEls.set(tab.id, el);
                onCleanup(() => tabEls.delete(tab.id));
              }}
              class={`tab ${win.tabs.activeTabId() === tab.id ? "tab-active" : ""}`}
              onClick={() => win.tabs.setActiveTabId(tab.id)}
              onDblClick={(e) => { e.stopPropagation(); setEditingTabId(tab.id); }}
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
                aria-label={`Close ${tab.title}`}
                onClick={(e) => { e.stopPropagation(); void win.tabs.closeTab(tab.id); }}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.stopPropagation(); void win.tabs.closeTab(tab.id); } }}
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
        onClick={() => void win.tabs.createTab()}
      >+</button>
    </div>
  );
}
