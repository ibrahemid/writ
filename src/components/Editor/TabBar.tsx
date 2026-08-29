import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { windowRegistry } from "../../stores/global/window-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { showToast } from "../Notifications/Toast";
import { abbreviateTitle } from "../../lib/buffer-name";
import { resolvePlatform } from "../../lib/platform";
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
  // Read per mount: the platform layer is written once at boot (ADR-030).
  const platform = resolvePlatform();
  const tabEls = new Map<string, HTMLButtonElement>();
  const tabs = () => bufferRegistry.activeTabs();

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
      void bufferRegistry
        .renameBuffer(tabId, trimmed)
        .catch(() => showToast("Could not rename the note", "error"));
    }
    setEditingTabId(null);
  }

  function handleRenameKeyDown(e: KeyboardEvent, tabId: string) {
    // The field is inside the strip, so every key it sees has to stop here:
    // an arrow that reached the strip would switch tabs, blur the field, and
    // commit a half-typed name.
    e.stopPropagation();
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

  /** One tab stop for the strip: the arrows move the selection inside it. */
  function handleArrowKey(e: KeyboardEvent, tabId: string) {
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    // A tab being renamed is a text field: the arrows belong to the caret.
    if (editingTabId() === tabId) return;
    const ids = tabs().map((tab) => tab.id);
    if (ids.length < 2) return;
    e.preventDefault();
    const from = ids.indexOf(tabId);
    const next = ids[(Math.max(from, 0) + step + ids.length) % ids.length];
    win.tabs.setActiveTabId(next);
    tabEls.get(next)?.focus();
  }

  /**
   * A close from the keyboard has to leave focus somewhere. The next tab takes
   * it; when the close drops the strip below two notes there is no strip left,
   * so the note takes it instead.
   */
  async function closeFromKeyboard(tabId: string) {
    const ids = tabs().map((tab) => tab.id);
    const index = ids.indexOf(tabId);
    const successor = ids[index + 1] ?? ids[index - 1];
    await win.tabs.closeTab(tabId);
    const remaining = tabs();
    // A refused close keeps the note open: focus stays on the tab holding it.
    if (remaining.some((tab) => tab.id === tabId)) {
      tabEls.get(tabId)?.focus();
      return;
    }
    if (remaining.length < 2 || successor === undefined) {
      win.editor.focusEditor();
      return;
    }
    tabEls.get(successor)?.focus();
  }

  // Hidden at one note (ADR-030 §5): a strip of one tab names what the window
  // already shows.
  return (
    <Show when={tabs().length > 1}>
      <div class="tabbar" data-platform={platform}>
        {/* The tablist owns its tabs: the anchor and the slot around each one
            are out of the accessibility tree, and the add control is a sibling
            of the list rather than a stray child of it. */}
        <div class="tabbar-tabs" role="tablist" aria-label="Open notes">
          <For each={tabs()}>
            {(tab) => {
              const isActive = () => win.tabs.activeTabId() === tab.id;
              const isEditing = () => editingTabId() === tab.id;
              return (
                <Tooltip label={tab.title} anchorRole="none">
                  <div
                    class={`tab ${isActive() ? "tab-active" : ""}`}
                    role="presentation"
                    onContextMenu={(e) => handleContextMenu(e, tab.id)}
                    onKeyDown={(e) => handleArrowKey(e, tab.id)}
                  >
                    {/* The field replaces the tab rather than sitting inside
                        it: a textbox is not a legal child of a tab. */}
                    <Show when={isEditing()} fallback={
                      <button
                        ref={(el) => {
                          tabEls.set(tab.id, el);
                          onCleanup(() => tabEls.delete(tab.id));
                        }}
                        type="button"
                        class="tab-label"
                        role="tab"
                        aria-selected={isActive()}
                        tabIndex={isActive() ? 0 : -1}
                        onClick={() => win.tabs.setActiveTabId(tab.id)}
                        onDblClick={(e) => { e.stopPropagation(); setEditingTabId(tab.id); }}
                      >
                        <span class="tab-title">{abbreviateTitle(tab.title)}</span>
                      </button>
                    }>
                      <input
                        ref={(el) => {
                          requestAnimationFrame(() => {
                            el.focus();
                            el.select();
                          });
                        }}
                        class="tab-rename-input"
                        aria-label="Rename note"
                        value={tab.title}
                        onBlur={(e) => handleRenameSubmit(tab.id, e.currentTarget.value)}
                        onKeyDown={(e) => handleRenameKeyDown(e, tab.id)}
                      />
                    </Show>
                    {/* A sibling of the tab, never a button nested inside one. */}
                    <button
                      type="button"
                      class="tab-close"
                      aria-label={`Close ${tab.title}`}
                      tabIndex={isActive() ? 0 : -1}
                      onClick={(e) => { e.stopPropagation(); void win.tabs.closeTab(tab.id); }}
                      onKeyDown={(e) => {
                        if (e.key !== "Enter" && e.key !== " ") return;
                        e.preventDefault();
                        e.stopPropagation();
                        void closeFromKeyboard(tab.id);
                      }}
                    >
                      <Icon name="x" size={12} />
                    </button>
                  </div>
                </Tooltip>
              );
            }}
          </For>
        </div>
        <Tooltip label="New tab">
          <button
            type="button"
            class="tab-add"
            aria-label="New tab"
            onClick={() => void win.tabs.createTab()}
          >
            <Icon name="plus" size={16} />
          </button>
        </Tooltip>
      </div>
    </Show>
  );
}
