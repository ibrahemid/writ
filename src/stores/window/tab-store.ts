import type { BufferDocument } from "../../types/buffer";
import type { BufferRegistry, OpenOutcome } from "../global/buffer-registry";
import { largeFileOpen } from "../global/large-file-open";
import { reportOpenOutcome, reportOpenOutcomes } from "../global/open-report";
import { flushAutosave, keepUnsavedForRecovery, type SaveFailure } from "../../services/autosave";
import { asSentence, formatSaveError } from "../../lib/save-error";
import { requestConfirm } from "../global/dialogs";
import { basename } from "../../lib/path";
import type { DownloadStore } from "./download-store";
import { saveStatusStore } from "../global/save-status";
import { createPaneStore, type PaneStore } from "./pane-store";
import { otherPane, type PaneId } from "../../lib/pane-layout";

export type TabStore = ReturnType<typeof createTabStore>;

// Per-window selected-tab state. The set of buffers is app-global
// (bufferRegistry); which panes show them and which one is in front is
// per-window (ADR-056). Tab-management operations are surfaced here so the
// panes track registry mutations atomically. `activeTabId` is the focused
// pane's tab in front.

export function createTabStore(deps: {
  registry: BufferRegistry;
  downloads: DownloadStore;
  // Optional, so a caller driving the tab store on its own does not have to
  // build an editor for it. Given one, a note whose tab has gone stops being
  // measured against its file.
  editor?: { noteClosed: (id: string) => void };
  // Given by the window; a caller driving the tab store on its own gets panes
  // of its own.
  panes?: PaneStore;
}) {
  const { registry, downloads, editor } = deps;
  const panes = deps.panes ?? createPaneStore();

  function openIds(): string[] {
    return registry.activeTabs().map((b) => b.id);
  }

  // The tabs open when the store last looked. A tab selected that is not among
  // them was opened by a caller outside the store (the licenses, the first
  // note), and joins the focused pane like a tab the store opened: no
  // placement reads as the left pane, so the layout cannot tell it apart.
  // Empty until the store first looks, which comes before focus can leave the
  // left pane.
  let seenOpen = new Set<string>();

  function noteOpenTabs(): void {
    seenOpen = new Set(openIds());
  }

  function activeTabId(): string | null {
    return panes.activeIn(panes.focused());
  }

  // Selecting a note's tab takes the pane back from a pending download, which
  // keeps its own tab. Clearing the selection (null) is what the download
  // branch of openFile does, and must leave the download showing. A tab
  // already open is selected in the pane that shows it; one just opened
  // (`fresh`, else not open when the store last looked) joins the focused pane.
  function setActiveTabId(id: string | null, options?: { fresh?: boolean }): void {
    if (id !== null) downloads.select(null);
    panes.select(id, options?.fresh ?? (id !== null && !seenOpen.has(id)));
    noteOpenTabs();
  }

  /** Selects `id` in `pane`, which takes focus: a click on that pane's strip. */
  function selectIn(pane: PaneId, id: string): void {
    downloads.select(null);
    panes.selectIn(pane, id);
    noteOpenTabs();
  }

  /** The tabs `pane` shows, in tab order. */
  function tabsIn(pane: PaneId): string[] {
    return panes.tabsIn(pane, openIds());
  }

  // A download's pane is the left one: the left pane holds tabs whenever any
  // are open, and its strip lists the downloads.
  function showDownloadPane(): void {
    panes.focus("left");
    panes.select(null, false);
  }

  // Called after every close path, on the ids that path aimed at. A tab the
  // registry refused to close is still open and keeps its record.
  //
  // Awaited rather than left running: text a save could not write is handed to
  // the unsaved-text journal here, and a close that races the quit is the case
  // that handover exists for.
  async function forgetClosed(ids: readonly string[]) {
    const open = new Set(registry.activeTabs().map((b) => b.id));
    noteOpenTabs();
    for (const id of ids) {
      if (open.has(id)) continue;
      editor?.noteClosed(id);
      saveStatusStore.forgetNote(id);
      await keepUnsavedForRecovery(id);
    }
  }

  async function loadAndActivate() {
    await registry.load();
    noteOpenTabs();
    const active = registry.activeTabs();
    panes.settle(active.map((b) => b.id));
    if (!activeTabId() && active.length > 0) {
      setActiveTabId(active[active.length - 1].id);
    }
  }

  /**
   * Puts the panes back as the last launch left them, before a tab is chosen.
   * A tab something already put in front (a file opened at launch) stays in
   * front.
   */
  function restorePanes(): void {
    const shown = activeTabId();
    const open = openIds();
    noteOpenTabs();
    panes.restore(open);
    if (shown !== null && open.includes(shown)) panes.select(shown, false);
  }

  async function createTab(title?: string): Promise<BufferDocument> {
    const doc = await registry.createBuffer(title);
    setActiveTabId(doc.id, { fresh: true });
    return doc;
  }

  async function newNote(): Promise<BufferDocument> {
    const doc = await registry.newNote();
    setActiveTabId(doc.id, { fresh: true });
    return doc;
  }

  async function newNoteInFolder(folder: string): Promise<BufferDocument> {
    const doc = await registry.newNoteInFolder(folder);
    setActiveTabId(doc.id, { fresh: true });
    return doc;
  }

  // Today's file may already be open, in which case the row that comes back
  // is that tab's, and selecting it is the whole of the command. Otherwise it
  // answers as an open of its path does, and is reported the same way.
  async function todaysNote(): Promise<BufferDocument | null> {
    const before = new Set(openIds());
    const outcome = await showOpened(await registry.todaysNote(), true, before);
    reportOpenOutcome(outcome);
    return outcome.kind === "opened" ? outcome.doc : null;
  }

  // The tab goes with the note. Selection moves to the survivor first, for the
  // same reason closeTab does it: a transient null active buffer recreates the
  // preview iframe, which hard-freezes the macOS webview.
  /** Moves the note to the Trash; answers the token its Undo takes, if any. */
  async function deleteNote(id: string): Promise<string | null> {
    leaveEveryPane(id);
    const undo = await registry.deleteNote(id);
    await forgetClosed([id]);
    return undo;
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

  function leaveEveryPane(id: string) {
    // Move the selection to the surviving tab BEFORE mutating buffer status.
    // closeBuffer flips the closed buffer to history, which synchronously
    // re-runs the active-buffer memo; if activeTabId still pointed at the
    // closed id, that memo would resolve to null for one flush before we
    // reselect, disposing and recreating the preview iframe element instead
    // of navigating its src. The destroy-then-recreate of a writ-preview://
    // iframe hard-freezes the macOS webview. Reselecting first keeps the
    // transition active->next (src navigation) or active->null only when no
    // tab survives (a single clean teardown), both safe. A pane the tab
    // leaves empty collapses here too, with focus on the pane that stays.
    //
    // The right pane lets go first, each pane asked afresh: the left pane
    // emptied while the right one still showed the tab would take the right
    // pane's tabs, this one in front again.
    for (const pane of ["right", "left"] as const) {
      if (panes.holds(pane, id)) panes.removeFromPane(pane, id, openIds());
    }
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
            `Could not save ${names[0]}`,
            `${asSentence(formatSaveError(failures.find((f) => f.bufferId === ids[0])?.error))} Closing the tab discards the unsaved text.`,
          )
        : await confirmDiscard(
            `Could not save ${ids.length} tabs`,
            `${names.join(", ")}\n\nClosing them discards the unsaved text.`,
          );
    if (!discard) return;
    if (ids.length === 1) {
      leaveEveryPane(ids[0]);
      await registry.closeBuffer(ids[0], { discard: true });
    } else {
      await registry.closeBuffers(ids, { discard: true });
    }
  }

  // The pane closing a tab: the one named, else the focused pane when it
  // shows the tab, else the pane that does.
  function closingPane(id: string, pane?: PaneId): PaneId {
    if (pane !== undefined) return pane;
    const shown = panes.panesOf(id);
    return shown.includes(panes.focused()) ? panes.focused() : (shown[0] ?? "left");
  }

  // The note's text is written before the selection moves: with no survivor,
  // moving it unmounts the editor, and a note whose write fails keeps its tab
  // without the selection ever leaving it. A tab the other pane still shows
  // only leaves this pane; the note stays open there.
  async function closeTab(id: string, pane?: PaneId): Promise<void> {
    const from = closingPane(id, pane);
    if (panes.panesOf(id).length > 1) {
      panes.removeFromPane(from, id, openIds());
      return;
    }
    const flushed = await flushAutosave(id);
    if (flushed.ok) {
      leaveEveryPane(id);
      const outcome = await registry.closeBuffer(id);
      if (outcome.closed) {
        await forgetClosed([id]);
        return;
      }
      reselectRefused(id);
      await discardFailed([id], outcome.failures);
    } else {
      await discardFailed([id], flushed.failures);
    }
    reselectRefused(id);
    await forgetClosed([id]);
  }

  // The other tabs of the pane `keepId` is in. One the other pane also shows
  // only leaves this pane.
  async function closeOtherTabs(keepId: string, pane?: PaneId): Promise<void> {
    const from = closingPane(keepId, pane);
    const others = panes.tabsIn(from, openIds()).filter((id) => id !== keepId);
    if (others.length === 0) return;
    // Reselect the surviving tab first for the same reason as closeTab: a
    // transient null active-buffer would recreate the preview iframe.
    panes.selectIn(from, keepId);
    downloads.select(null);
    const ids: string[] = [];
    for (const id of others) {
      if (panes.panesOf(id).length > 1) panes.removeFromPane(from, id, openIds());
      else ids.push(id);
    }
    if (ids.length === 0) return;
    const outcome = await registry.closeBuffers(ids);
    await discardFailed(outcome.failedIds, outcome.failures);
    await forgetClosed(ids);
    panes.settle(openIds());
  }

  // `closeBuffers` writes every note before any leaves the active set, which
  // is what moves the selection here.
  /** Closes every tab and answers the ones that moved to Recent, in tab order. */
  async function closeAllTabs(): Promise<string[]> {
    const toClose = registry.activeTabs();
    if (toClose.length === 0) {
      setActiveTabId(null);
      return [];
    }
    const ids = toClose.map((b) => b.id);
    const outcome = await registry.closeBuffers(ids);
    await discardFailed(outcome.failedIds, outcome.failures);
    await forgetClosed(ids);
    const remaining = registry.activeTabs();
    panes.settle(remaining.map((b) => b.id));
    if (!activeTabId()) setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
    const open = new Set(remaining.map((b) => b.id));
    return ids.filter((id) => !open.has(id));
  }

  /**
   * Opens closed tabs again, in the order given, and puts `front` in front
   * when it came back. A file that cannot be opened again is reported once
   * with the rest.
   */
  async function reopenTabs(ids: readonly string[], front: string | null): Promise<void> {
    const outcomes: OpenOutcome[] = [];
    for (const id of ids) {
      outcomes.push(await showOpened(await registry.restoreBuffer(id), false));
    }
    reportOpenOutcomes(outcomes);
    const open = registry.activeTabs().map((b) => b.id);
    const back = ids.filter((id) => open.includes(id));
    if (back.length === 0) return;
    setActiveTabId(front !== null && back.includes(front) ? front : back[back.length - 1], { fresh: true });
  }

  // A closed file reopens in front, or says why it did not.
  async function restoreFromHistory(id: string): Promise<BufferDocument | null> {
    const before = new Set(openIds());
    const outcome = await showOpened(await registry.restoreBuffer(id), true, before);
    reportOpenOutcome(outcome);
    return outcome.kind === "opened" ? outcome.doc : null;
  }

  // A tab the last session left open, shown before its file was read. Its
  // open asks or refuses as reopening a closed file does and reports nothing:
  // a refusal stays in the tab, which says why. An open the person answered
  // brings the tab in front.
  async function openUnreadTab(id: string): Promise<OpenOutcome> {
    const outcome = await registry.openUnreadTab(id);
    if (outcome.kind === "opened") return showOpened(outcome, true);
    return outcome;
  }

  // Opens a path and answers what the open came to, reporting nothing. A file
  // whose bytes are not on this machine goes to the download store, which
  // opens it again once they arrive. `activate` is false for a note that
  // finished downloading while the person was reading another one: it gets
  // its tab without taking the screen away from them.
  async function openPath(
    path: string,
    options?: { activate?: boolean },
  ): Promise<OpenOutcome> {
    const before = new Set(openIds());
    return showOpened(await registry.openFile(path), options?.activate ?? true, before);
  }

  // What an open's answer does to the window: a file that opened gets its tab,
  // one still downloading gets the download's pane, and Cancel or a refusal
  // change nothing. `before` is the tabs open before the open asked: a tab not
  // among them was opened by it and joins the focused pane.
  async function showOpened(
    outcome: OpenOutcome,
    activate: boolean,
    before?: ReadonlySet<string>,
  ): Promise<OpenOutcome> {
    if (outcome.kind === "declined" || outcome.kind === "refused") return outcome;
    // The note on screen is written before the selection leaves it: the
    // download's pane replaces the editor outright.
    const shown = activeTabId();
    if (activate && shown !== null) await flushAutosave(shown);
    if (outcome.kind === "not-downloaded") {
      // Nothing is read and no buffer exists, so no tab can be active behind
      // the download's own pane.
      if (activate) showDownloadPane();
      await downloads.start({
        path: outcome.path,
        title: basename(outcome.path),
        provider: outcome.provider,
      });
      return outcome;
    }
    if (activate) {
      downloads.select(null);
      setActiveTabId(outcome.doc.id, { fresh: before !== undefined && !before.has(outcome.doc.id) });
    } else {
      // No editor is about to read it, so the progress ends with the open.
      largeFileOpen.finishFor(outcome.doc.id);
      noteOpenTabs();
    }
    return outcome;
  }

  // Opens a path and says what went wrong, if anything did. Null for every
  // answer that opened no tab: Cancel, a refusal, a file still downloading.
  async function openFile(
    path: string,
    options?: { activate?: boolean },
  ): Promise<BufferDocument | null> {
    const outcome = await openPath(path, options);
    reportOpenOutcome(outcome);
    return outcome.kind === "opened" ? outcome.doc : null;
  }

  async function openFileDialog(): Promise<BufferDocument | null> {
    const path = await registry.showOpenFileDialog();
    if (!path) return null;
    return openFile(path);
  }

  /** Shows the focused pane's tab in the other pane too, which takes focus. */
  function splitRight(): void {
    panes.splitToOther();
  }

  /** One pane again, holding every tab. */
  function closeSplit(): void {
    panes.closeSplit();
  }

  function focusOtherPane(): void {
    if (panes.isSplit()) panes.focus(otherPane(panes.focused()));
  }

  /**
   * Moves the focused pane's tab in front, or `id`, to the other pane. A pane's
   * only tab stays where it is while the other pane is empty: moving it would
   * leave the panes as they were.
   */
  function moveToOtherPane(id?: string, from: PaneId = panes.focused()): void {
    const tab = id ?? panes.activeIn(from);
    if (!tab || !panes.holds(from, tab)) return;
    const open = openIds();
    if (!panes.isSplit() && panes.tabsIn(from, open).length < 2) return;
    downloads.select(null);
    panes.moveToOther(from, tab, open);
  }

  return {
    activeTabId,
    setActiveTabId,
    selectIn,
    tabsIn,
    showDownloadPane,
    restorePanes,
    splitRight,
    closeSplit,
    focusOtherPane,
    moveToOtherPane,
    loadAndActivate,
    createTab,
    newNote,
    newNoteInFolder,
    todaysNote,
    deleteNote,
    closeTab,
    closeOtherTabs,
    closeAllTabs,
    reopenTabs,
    restoreFromHistory,
    openUnreadTab,
    openPath,
    openFile,
    openFileDialog,
  };
}
