import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import type { BufferRegistry } from "../global/buffer-registry";
import { keepUnsavedForRecovery, type SaveFailure } from "../../services/autosave";
import { asSentence, formatSaveError } from "../../lib/save-error";
import { requestConfirm } from "../../components/ConfirmDialog/ConfirmDialog";
import { saveStatusStore } from "../global/save-status";

export type TabStore = ReturnType<typeof createTabStore>;

// Per-window selected-tab state. The set of buffers is app-global
// (bufferRegistry); which one is focused is per-window. Tab-management
// operations are surfaced here so the per-window activeTabId tracks
// registry mutations atomically.

export function createTabStore(deps: {
  registry: BufferRegistry;
  // Optional, so a caller driving the tab store on its own does not have to
  // build an editor for it. Given one, a note whose tab has gone stops being
  // measured against its file.
  editor?: { noteClosed: (id: string) => void };
}) {
  const { registry, editor } = deps;
  const [activeTabId, setActiveTabId] = createSignal<string | null>(null);

  // Called after every close path, on the ids that path aimed at. A tab the
  // registry refused to close is still open and keeps its record.
  //
  // Awaited rather than left running: text a save could not write is handed to
  // the recovery snapshot here, and a close that races the quit is the case
  // that handover exists for.
  async function forgetClosed(ids: readonly string[]) {
    const open = new Set(registry.activeTabs().map((b) => b.id));
    for (const id of ids) {
      if (open.has(id)) continue;
      editor?.noteClosed(id);
      saveStatusStore.forgetNote(id);
      await keepUnsavedForRecovery(id);
    }
  }

  async function loadAndActivate() {
    await registry.load();
    const active = registry.activeTabs();
    const currentId = activeTabId();
    if (currentId && !active.find((b) => b.id === currentId)) {
      setActiveTabId(active.length > 0 ? active[active.length - 1].id : null);
    } else if (!currentId && active.length > 0) {
      setActiveTabId(active[active.length - 1].id);
    }
  }

  async function createTab(title?: string): Promise<BufferDocument> {
    const doc = await registry.createBuffer(title);
    setActiveTabId(doc.id);
    return doc;
  }

  async function newNote(): Promise<BufferDocument> {
    const doc = await registry.newNote();
    setActiveTabId(doc.id);
    return doc;
  }

  // The tab goes with the note. Selection moves to the survivor first, for the
  // same reason closeTab does it: a transient null active buffer recreates the
  // preview iframe, which hard-freezes the macOS webview.
  async function deleteNote(id: string): Promise<void> {
    selectSurvivor(id);
    await registry.deleteNote(id);
    await forgetClosed([id]);
  }

  // A buffer on a volume that never accepts a write would otherwise hold its
  // tab open for the rest of the session. Name what went wrong and let the user
  // decide, with the safe answer focused.
  async function confirmDiscard(title: string, message: string): Promise<boolean> {
    return requestConfirm({
      title,
      message,
      confirmLabel: "Close and lose changes",
      cancelLabel: "Keep open",
      danger: true,
      defaultAction: "cancel",
    });
  }

  function bufferTitle(id: string): string {
    return registry.buffers().find((b) => b.id === id)?.title ?? id;
  }

  function selectSurvivor(id: string) {
    // Move the selection to the surviving tab BEFORE mutating buffer status.
    // closeBuffer flips the closed buffer to history, which synchronously
    // re-runs the active-buffer memo; if activeTabId still pointed at the
    // closed id, that memo would resolve to null for one flush before we
    // reselect, disposing and recreating the preview iframe element instead
    // of navigating its src. The destroy-then-recreate of a writ-preview://
    // iframe hard-freezes the macOS webview. Reselecting first keeps the
    // transition active->next (src navigation) or active->null only when no
    // tab survives (a single clean teardown), both safe.
    if (activeTabId() !== id) return;
    const remaining = registry.activeTabs().filter((b) => b.id !== id);
    setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
  }

  function reselectRefused(id: string) {
    // The registry refuses to close a buffer whose text could not be written.
    // Put the user back on the tab that still holds it.
    if (registry.activeTabs().some((b) => b.id === id)) {
      setActiveTabId(id);
    }
  }

  async function discardFailed(ids: string[], failures: SaveFailure[]): Promise<void> {
    if (ids.length === 0) return;
    const names = ids.map(bufferTitle);
    const discard =
      ids.length === 1
        ? await confirmDiscard(
            `Couldn't save ${names[0]}`,
            `${asSentence(formatSaveError(failures.find((f) => f.bufferId === ids[0])?.error))} Closing the tab discards the unsaved text.`,
          )
        : await confirmDiscard(
            `Couldn't save ${ids.length} tabs`,
            `${names.join(", ")}\n\nClosing them discards the unsaved text.`,
          );
    if (!discard) return;
    if (ids.length === 1) {
      selectSurvivor(ids[0]);
      await registry.closeBuffer(ids[0], { discard: true });
    } else {
      await registry.closeBuffers(ids, { discard: true });
    }
  }

  async function closeTab(id: string): Promise<void> {
    selectSurvivor(id);
    const outcome = await registry.closeBuffer(id);
    if (outcome.closed) {
      await forgetClosed([id]);
      return;
    }
    reselectRefused(id);
    await discardFailed([id], outcome.failures);
    reselectRefused(id);
    await forgetClosed([id]);
  }

  async function closeOtherTabs(keepId: string): Promise<void> {
    const toClose = registry.activeTabs().filter((b) => b.id !== keepId);
    if (toClose.length === 0) return;
    // Reselect the surviving tab first for the same reason as closeTab: a
    // transient null active-buffer would recreate the preview iframe.
    setActiveTabId(keepId);
    const ids = toClose.map((b) => b.id);
    const outcome = await registry.closeBuffers(ids);
    await discardFailed(outcome.failedIds, outcome.failures);
    await forgetClosed(ids);
  }

  async function closeAllTabs(): Promise<void> {
    const toClose = registry.activeTabs();
    if (toClose.length === 0) {
      setActiveTabId(null);
      return;
    }
    const ids = toClose.map((b) => b.id);
    const outcome = await registry.closeBuffers(ids);
    await discardFailed(outcome.failedIds, outcome.failures);
    await forgetClosed(ids);
    const remaining = registry.activeTabs();
    setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
  }

  async function restoreFromHistory(id: string): Promise<void> {
    await registry.restoreBuffer(id);
    setActiveTabId(id);
  }

  async function openFile(path: string): Promise<BufferDocument> {
    const { doc } = await registry.openFile(path);
    setActiveTabId(doc.id);
    return doc;
  }

  async function openFileDialog(): Promise<BufferDocument | null> {
    const path = await registry.showOpenFileDialog();
    if (!path) return null;
    return openFile(path);
  }

  return {
    activeTabId,
    setActiveTabId,
    loadAndActivate,
    createTab,
    newNote,
    deleteNote,
    closeTab,
    closeOtherTabs,
    closeAllTabs,
    restoreFromHistory,
    openFile,
    openFileDialog,
  };
}
