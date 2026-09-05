import { For, createSignal, createEffect, onCleanup, Show } from "solid-js";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { windowRegistry } from "../../stores/global/window-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { abbreviateTitle } from "../../lib/buffer-name";
import {
  confirmAndDeleteNote,
  noteIsDeletable,
  saveCopyOfNote,
  showInFileManagerLabel,
  showNoteInFileManager,
} from "../../lib/note-actions";
import { formatRenameError } from "../../lib/save-error";
import { showToast } from "../Notifications/Toast";
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
        label: "Save a Copy…",
        action: () => void saveCopyOfNote(tabId),
        separator: true,
      },
      {
        label: "Delete",
        action: () => void confirmAndDeleteNote(tabId),
        disabled: !noteIsDeletable(tabId),
        danger: true,
      },
      { label: "Close Tab", action: () => void win.tabs.closeTab(tabId), separator: true },
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
        <For each={win.downloads.pending()}>
          {(download) => (
            <button
              class={`tab tab-download ${
                win.downloads.selectedPath() === download.path ? "tab-active" : ""
              }`}
              onClick={() => selectDownload(download.path)}
              title={download.title}
            >
              <span class="tab-title">{abbreviateTitle(download.title)}</span>
              <span class="tab-download-marker">{markerFor(download.state)}</span>
              <span
                class="tab-close"
                role="button"
                tabIndex={0}
                aria-label={`${download.state === "downloading" ? "Cancel" : "Close"} ${download.title}`}
                onClick={(e) => { e.stopPropagation(); void win.downloads.dismiss(download.path); }}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.stopPropagation(); void win.downloads.dismiss(download.path); } }}
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
        aria-label="New note"
        title="New note"
        onClick={() => void win.tabs.newNote()}
      >+</button>
    </div>
  );
}
