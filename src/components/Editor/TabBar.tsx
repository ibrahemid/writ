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
import {
  confirmAndDeleteNote,
  noteIsDeletable,
  saveCopyOfNote,
  showInFileManagerLabel,
  showNoteInFileManager,
} from "../../lib/note-actions";
import { formatRenameError } from "../../lib/save-error";
import SaveMarker from "../SaveMarker/SaveMarker";
import type { PendingDownload } from "../../stores/window/download-store";
import { logFailure } from "../../lib/log";
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

  // Renaming a tab renames the note's file, so it can be stopped: an empty
  // name, a name the folder already holds, a file something else rewrote. The
  // backend decides all three, and the answer has to reach the person who
  // typed the name rather than being dropped on the floor.
  function handleRenameSubmit(tabId: string, value: string) {
    setEditingTabId(null);
    void bufferRegistry.renameBuffer(tabId, value).catch((error) => {
      logFailure("a note could not be renamed");
      showToast(formatRenameError(error), "error");
    });
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

  // A note that is not here yet has a tab but no buffer, so nothing can be the
  // active tab behind its pane.
  function selectDownload(path: string) {
    win.downloads.select(path);
    win.tabs.setActiveTabId(null);
  }

  // Text, not an animation: a download reports nothing Writ could animate
  // honestly, and a tab that moves pulls the eye off the note being written.
  function markerFor(state: PendingDownload["state"]): string {
    return state === "downloading" ? "downloading" : "not downloaded";
  }

  function handleContextMenu(e: MouseEvent, tabId: string) {
    e.preventDefault();
    const onDisk = bufferRegistry.buffers().some((b) => b.id === tabId && b.source_path);
    showContextMenu(e.clientX, e.clientY, [
      { label: "Rename", action: () => setEditingTabId(tabId) },
      {
        label: showInFileManagerLabel(),
        action: () => void showNoteInFileManager(tabId),
        disabled: !onDisk,
      },
      {
        label: "Save a copy…",
        action: () => void saveCopyOfNote(tabId),
        separator: true,
      },
      {
        label: "Delete",
        action: () => void confirmAndDeleteNote(tabId),
        disabled: !noteIsDeletable(tabId),
        danger: true,
      },
      { label: "Close tab", action: () => void win.tabs.closeTab(tabId), separator: true },
      { label: "Close other tabs", action: () => void win.tabs.closeOtherTabs(tabId) },
      { label: "Close all tabs", action: () => void win.tabs.closeAllTabs(), separator: true, danger: true },
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
  // already shows. A note waiting on its bytes has a tab and no buffer, so the
  // strip stays up for it however few notes are open.
  return (
    <Show when={tabs().length > 1 || win.downloads.pending().length > 0}>
      <div class="tabbar" data-platform={platform}>
        {/* The tablist owns its tabs: the anchor and the slot around each one
            are out of the accessibility tree, and the add control is a sibling
            of the list rather than a stray child of it. */}
        <div class="tabbar-tabs" role="tablist" aria-label="Open notes">
          <For each={tabs()}>
            {(tab) => {
              const isActive = () => win.tabs.activeTabId() === tab.id;
              const isEditing = () => editingTabId() === tab.id;
              const isRemoved = () => win.editor.isRemovedOnDisk(tab.id);
              // A tab whose file is gone says so in words as well as in the
              // strike-through, on both the pointer and the screen-reader path.
              const label = () => (isRemoved() ? `${tab.title} (deleted)` : tab.title);
              return (
                <Tooltip label={label()} anchorRole="none">
                  <div
                    class={`tab ${isActive() ? "tab-active" : ""}`}
                    classList={{ "tab-removed": isRemoved() }}
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
                        aria-label={isRemoved() ? label() : undefined}
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
                        data-writ-focus-silent
                        aria-label="Rename note"
                        value={tab.title}
                        onBlur={(e) => handleRenameSubmit(tab.id, e.currentTarget.value)}
                        onKeyDown={(e) => handleRenameKeyDown(e, tab.id)}
                      />
                    </Show>
                    <SaveMarker noteId={tab.id} />
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
          <For each={win.downloads.pending()}>
            {(download) => {
              const isSelected = () => win.downloads.selectedPath() === download.path;
              const dismissLabel = () =>
                `${download.state === "downloading" ? "Cancel" : "Close"} ${download.title}`;
              return (
                <Tooltip label={download.title} anchorRole="none">
                  <div
                    class={`tab tab-download ${isSelected() ? "tab-active" : ""}`}
                    role="presentation"
                  >
                    <button
                      type="button"
                      class="tab-label"
                      role="tab"
                      aria-selected={isSelected()}
                      tabIndex={isSelected() ? 0 : -1}
                      onClick={() => selectDownload(download.path)}
                    >
                      <span class="tab-title">{abbreviateTitle(download.title)}</span>
                      <span class="tab-download-marker">{markerFor(download.state)}</span>
                    </button>
                    <button
                      type="button"
                      class="tab-close"
                      aria-label={dismissLabel()}
                      tabIndex={isSelected() ? 0 : -1}
                      onClick={(e) => { e.stopPropagation(); void win.downloads.dismiss(download.path); }}
                      onKeyDown={(e) => {
                        if (e.key !== "Enter" && e.key !== " ") return;
                        e.preventDefault();
                        e.stopPropagation();
                        void win.downloads.dismiss(download.path);
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
        <Tooltip label="New note">
          <button
            type="button"
            class="tab-add"
            aria-label="New note"
            onClick={() => void win.tabs.newNote()}
          >
            <Icon name="plus" size={16} />
          </button>
        </Tooltip>
      </div>
    </Show>
  );
}
