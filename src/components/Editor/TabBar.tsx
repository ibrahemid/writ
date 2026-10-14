import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { windowRegistry } from "../../stores/global/window-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { isMenuOpenFor, showAnchoredMenu, showContextMenu, type MenuItem } from "../ContextMenu/ContextMenu";
import { showToast } from "../Notifications/Toast";
import { abbreviateTitle } from "../../lib/buffer-name";
import { tierName } from "../../lib/tier-explainer";
import { configStore } from "../../stores/global/config";
import { saveStatusStore } from "../../stores/global/save-status";
import { notesStore } from "../../stores/global/notes";
import { workspaceStore } from "../../stores/global/workspace";
import { clipboardStore } from "../../stores/global/clipboard";
import { executeCommand } from "../../commands/registry";
import { commandChord, withChord } from "../../lib/command-chord";
import { pathInRoots, tabFolderHints } from "../../lib/tab-labels";
import { displayFileName, renameSeed } from "../../lib/display-name";
import { resolvePlatform } from "../../lib/platform";
import {
  confirmAndDeleteNote,
  noteIsDeletable,
  renameNoteAndLinks,
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

  let listRef: HTMLDivElement | undefined;
  let overflowButton: HTMLButtonElement | undefined;
  const [clipped, setClipped] = createSignal({ start: false, end: false });
  const folderHints = createMemo(() => tabFolderHints(tabs()));

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

  function measureClip() {
    const el = listRef;
    if (!el) return;
    const max = el.scrollWidth - el.clientWidth;
    const start = max > 1 && el.scrollLeft > 1;
    const end = max > 1 && el.scrollLeft < max - 1;
    setClipped((prev) => (prev.start === start && prev.end === end ? prev : { start, end }));
  }

  onMount(() => {
    measureClip();
    if (!listRef || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => measureClip());
    observer.observe(listRef);
    onCleanup(() => observer.disconnect());
  });

  createEffect(() => {
    tabs();
    folderHints();
    win.downloads.pending();
    const frame = requestAnimationFrame(measureClip);
    onCleanup(() => cancelAnimationFrame(frame));
  });

  function handleWheel(e: WheelEvent) {
    const el = listRef;
    if (!el || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    if (el.scrollWidth <= el.clientWidth) return;
    e.preventDefault();
    el.scrollLeft += e.deltaY;
    measureClip();
  }

  function shownName(title: string): string {
    return abbreviateTitle(displayFileName(title));
  }

  function markerOf(id: string): MenuItem["marker"] {
    const state = saveStatusStore.stateOf(id);
    if (state === "failed") return "failed";
    return state === "dirty" || state === "saving" ? "unsaved" : undefined;
  }

  function openOverflowMenu() {
    if (!overflowButton) return;
    const hints = folderHints();
    const active = win.tabs.activeTabId();
    const items: MenuItem[] = tabs().map((tab) => {
      const hint = hints.get(tab.id);
      return {
        label: hint ? `${shownName(tab.title)} · ${hint}` : shownName(tab.title),
        checked: tab.id === active,
        marker: markerOf(tab.id),
        action: () => win.tabs.setActiveTabId(tab.id),
      };
    });
    for (const download of win.downloads.pending()) {
      items.push({
        label: `${shownName(download.title)} · ${markerFor(download.state)}`,
        checked: win.downloads.selectedPath() === download.path,
        action: () => selectDownload(download.path),
      });
    }
    showAnchoredMenu(overflowButton.getBoundingClientRect(), items, overflowButton);
  }

  // Renaming a tab renames the note's file, so it can be stopped: an empty
  // name, a name the folder already holds, a file something else rewrote. The
  // backend decides all three, and the answer has to reach the person who
  // typed the name rather than being dropped on the floor.
  //
  // Enter and Escape both close the field, and a focused field that goes away
  // is blurred, which lands here too. Only a field still open for this tab
  // commits, so Enter renames once and Escape renames nothing.
  function handleRenameSubmit(tabId: string, value: string) {
    if (editingTabId() !== tabId) return;
    setEditingTabId(null);
    void renameNoteAndLinks(tabId, value).catch((error) => {
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
    const path = bufferRegistry.buffers().find((b) => b.id === tabId)?.source_path ?? null;
    showContextMenu(e.clientX, e.clientY, [
      { label: "Rename", kbd: commandChord("note.rename"), action: () => setEditingTabId(tabId) },
      {
        label: showInFileManagerLabel(),
        action: () => void showNoteInFileManager(tabId),
        disabled: path === null,
      },
      {
        label: "Copy path",
        action: () => {
          if (path === null) return;
          void clipboardStore.copyText(path).catch(() => logFailure("a file path could not be copied"));
        },
        disabled: path === null,
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
      {
        label: "Close tab",
        kbd: commandChord("buffer.close"),
        action: () => void win.tabs.closeTab(tabId),
        separator: true,
      },
      { label: "Close other tabs", action: () => void win.tabs.closeOtherTabs(tabId) },
      {
        label: "Reopen closed tab",
        kbd: commandChord("history.restoreLast"),
        action: () => void executeCommand("history.restoreLast"),
        disabled: bufferRegistry.historyList().length === 0,
      },
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
   * it; a close that leaves no tab at all hands focus to the editor.
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
    if (remaining.length === 0 || successor === undefined) {
      win.editor.focusEditor();
      return;
    }
    tabEls.get(successor)?.focus();
  }

  return (
    <div class="tabbar" data-platform={platform}>
      {/* The tablist owns its tabs: the anchor and the slot around each one
          are out of the accessibility tree, and the add control is a sibling
          of the list rather than a stray child of it. */}
      <div
        ref={listRef}
        class="tabbar-tabs"
        classList={{ "is-clipped-start": clipped().start, "is-clipped-end": clipped().end }}
        role="tablist"
        aria-label="Open files"
        onScroll={measureClip}
        onWheel={handleWheel}
      >
        <For each={tabs()}>
          {(tab) => {
            const isActive = () => win.tabs.activeTabId() === tab.id;
            const isEditing = () => editingTabId() === tab.id;
            const isRemoved = () => win.editor.isRemovedOnDisk(tab.id);
            const hint = () => folderHints().get(tab.id);
            const label = () => (isRemoved() ? `${tab.title} (deleted)` : tab.title);
            const accessibleName = () => {
              const folder = hint();
              if (folder === undefined) return isRemoved() ? label() : undefined;
              return isRemoved() ? `${tab.title} in ${folder} (deleted)` : `${tab.title} in ${folder}`;
            };
            const tooltip = () => {
              const where = tab.source_path
                ? pathInRoots(tab.source_path, [workspaceStore.root(), notesStore.root()])
                : tab.title;
              const named = isRemoved() ? `${where} (deleted)` : where;
              const tier = configStore.config().editor.status_bar ? null : tierName(win.editor.tierOf(tab.id));
              return tier === null ? named : `${named} · ${tier}`;
            };
            return (
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
                  <Tooltip label={tooltip()} anchorRole="none">
                    <button
                      ref={(el) => {
                        tabEls.set(tab.id, el);
                        onCleanup(() => tabEls.delete(tab.id));
                      }}
                      type="button"
                      class="tab-label"
                      role="tab"
                      aria-selected={isActive()}
                      aria-label={accessibleName()}
                      tabIndex={isActive() ? 0 : -1}
                      onClick={() => win.tabs.setActiveTabId(tab.id)}
                      onDblClick={(e) => { e.stopPropagation(); setEditingTabId(tab.id); }}
                    >
                      <span class="tab-title">{shownName(tab.title)}</span>
                      <Show when={hint()}>
                        {(folder) => <span class="tab-folder">{folder()}</span>}
                      </Show>
                    </button>
                  </Tooltip>
                }>
                  <input
                    ref={(el) => {
                      const seed = renameSeed(tab.title);
                      requestAnimationFrame(() => {
                        el.focus();
                        // The stem only: typing over the selection keeps
                        // the format the file already has.
                        el.setSelectionRange(0, seed.selectionEnd);
                      });
                    }}
                    class="tab-rename-input"
                    data-writ-focus-silent
                    aria-label="Rename file"
                    value={renameSeed(tab.title).value}
                    onBlur={(e) => handleRenameSubmit(tab.id, e.currentTarget.value)}
                    onKeyDown={(e) => handleRenameKeyDown(e, tab.id)}
                  />
                </Show>
                <SaveMarker noteId={tab.id} />
                {/* A sibling of the tab, never a button nested inside one. */}
                <Tooltip label={withChord("Close", "buffer.close")} anchorRole="none">
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
                </Tooltip>
              </div>
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
                    <span class="tab-title">{shownName(download.title)}</span>
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
      <Show when={clipped().start || clipped().end}>
        <Tooltip label="Open files">
          <button
            ref={overflowButton}
            type="button"
            class="tab-overflow"
            aria-label="Open files"
            aria-haspopup="menu"
            aria-expanded={isMenuOpenFor(overflowButton)}
            onClick={openOverflowMenu}
          >
            <Icon name="caret-down" size={14} />
          </button>
        </Tooltip>
      </Show>
      <Tooltip label={withChord("New file", "note.new")}>
        <button
          type="button"
          class="tab-add"
          aria-label="New file"
          onClick={() => void win.tabs.newNote()}
        >
          <Icon name="plus" size={16} />
        </button>
      </Tooltip>
    </div>
  );
}
