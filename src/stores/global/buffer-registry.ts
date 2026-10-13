import { createSignal, createMemo, createRoot } from "solid-js";
import type { BufferDocument, FileOpenMode, FileOpenResult } from "../../types/buffer";
import * as api from "../../services/tauri";
import { cancelAutosave, flushAutosave, type SaveFailure } from "../../services/autosave";
import { logFailure } from "../../lib/log";
import { basename } from "../../lib/path";
import {
  ERR_OPEN_TOO_LARGE,
  MAX_OPEN_BYTES,
  openFailureReason,
  refusalReason,
  tooLargeReason,
} from "../../lib/open-failure";
import { leadingCodeOf } from "../../lib/read-failure";
import { THRESHOLD_NORMAL_BYTES } from "../../editor/large-file";
import { largeFileOpen } from "./large-file-open";
import { unreadTabs } from "./unread-tabs";

/**
 * What an open of a path came to. Every open has one of these answers and
 * none of them is thrown:
 *
 * - `opened`: the file has a row, new or (`existed`) already in a tab.
 * - `declined`: the file is over the size Writ opens without asking and the
 *   person said Cancel. Nothing was read, and nothing is said about it.
 * - `refused`: the file was not opened; `reason` is the second half of
 *   "Couldn't open <name>: ".
 * - `not-downloaded`: a sync provider has not put the bytes on this machine.
 *   The file is downloaded first and opened again once the bytes are here.
 */
export type OpenOutcome =
  | { kind: "opened"; doc: BufferDocument; existed: boolean; mode: FileOpenMode }
  | { kind: "declined"; path: string }
  | { kind: "refused"; path: string; reason: string }
  | { kind: "not-downloaded"; path: string; provider: string | null };

/** An open a caller has to treat as a failure was refused, with the reason. */
export class OpenRefusedError extends Error {
  constructor(
    readonly path: string,
    readonly reason: string,
  ) {
    super(`the file was not opened: ${reason}`);
    this.name = "OpenRefusedError";
  }
}

/**
 * A tab whose file Writ has not read was asked for its text before its open
 * answered: one restored unread at launch, or one whose read found the file
 * past the size Writ opens. Its file is read once its open answers, and never
 * past that size.
 */
export class UnreadTabReadError extends Error {
  constructor(readonly bufferId: string) {
    super("a tab whose file Writ has not read is read only once its open answers");
    this.name = "UnreadTabReadError";
  }
}

// The mode a row restored at launch is shown in. Rust classified it when it was
// first opened and nobody asked since, so the row's size and read-only flag
// say the rest; the confirm tier is a question asked before a read, and an
// open file of that size is a large file.
function modeOfRow(doc: BufferDocument): FileOpenMode {
  if (doc.read_only) return { kind: "Binary" };
  if (doc.size_bytes > THRESHOLD_NORMAL_BYTES) return { kind: "LargeFile" };
  return { kind: "Normal" };
}

export interface CloseOutcome {
  closed: boolean;
  failures: SaveFailure[];
}

export interface BulkCloseOutcome {
  closedIds: string[];
  failedIds: string[];
  failures: SaveFailure[];
}

export interface CloseOptions {
  // Close without writing: the caller has told the user the text is about to be
  // lost and the user chose to lose it. Without this a buffer on a volume that
  // never accepts a write can never be closed.
  discard?: boolean;
}

// Singleton — app-global, not window-scoped (ADR-009 E3).
// The set of buffers is shared across every window; active-tab focus per
// window lives in stores/window/tab-store.ts.

function createBufferRegistry() {
  const [buffers, setBuffers] = createSignal<BufferDocument[]>([]);
  // The mode each open reported, by row: a second open of the same file
  // answers what Rust classified rather than a guess from the row's size.
  const openModes = new Map<string, FileOpenMode>();

  const activeTabs = createMemo(() =>
    buffers().filter((b) => b.status === "active"),
  );
  const historyList = createMemo(() =>
    buffers().filter((b) => b.status === "history"),
  );

  async function load() {
    const [active, history] = await Promise.all([
      api.listActiveBuffers(),
      api.listHistory(),
    ]);
    setBuffers([...active, ...history]);
  }

  // The tabs the last session left open. Rust measures each tab's file while
  // listing it, so a file that grew past the size Writ opens without asking
  // while Writ was closed stays unread here, before any tab is shown.
  async function loadSession(): Promise<void> {
    await load();
    unreadTabs.takeRestored(activeTabs());
  }

  async function createBuffer(title?: string): Promise<BufferDocument> {
    await flushAutosave();
    const doc = await api.createBuffer(title);
    setBuffers((prev) =>
      prev.find((b) => b.id === doc.id) ? prev : [...prev, doc],
    );
    return doc;
  }

  // A note the tab bar has not seen yet, kept once: a row that is already in
  // the set is left as it stands, because the caller that put it there is the
  // one holding the note.
  function hold(doc: BufferDocument): void {
    setBuffers((prev) =>
      prev.find((b) => b.id === doc.id) ? prev : [...prev, doc],
    );
  }

  // A new note is a file in the notes folder before anything else happens, so
  // it is in Finder immediately rather than on the first keystroke.
  async function newNote(): Promise<BufferDocument> {
    await flushAutosave();
    const doc = await api.newNote();
    hold(doc);
    return doc;
  }

  // A note another call already minted, taken into the set the tab bar reads.
  // The first launch is the one caller: `finish_first_run` writes the file and
  // answers with its row, so there is nothing left here but to hold it.
  function adoptDocument(doc: BufferDocument): void {
    hold(doc);
  }

  // One file a day: today's is opened when it is already there and made when
  // it is not, and the row that comes back is the only honest name for it. A
  // file that was closed comes back active, so the entry is replaced rather
  // than left as the tab bar last saw it. The open answers as any open does,
  // so a day's file over the size Writ opens without asking is asked about.
  async function todaysNote(): Promise<OpenOutcome> {
    await flushAutosave();
    const today = await api.todaysNote();
    if (today.opened.mode.kind === "LargeFileConfirm") {
      return openAfterAsking(today.path, today.opened.size_bytes);
    }
    const unread = unreadTabAnswered(today.opened);
    if (unread !== null) return openUnreadTab(unread);
    return registerOpenResult(today.opened, today.path);
  }

  // Trashing is not undoable from inside Writ, so the pending write is dropped
  // rather than flushed: writing text into a file that is about to leave would
  // put it in the Trash too.
  async function deleteNote(id: string): Promise<void> {
    cancelAutosave(id);
    await api.deleteNote(id);
    void api.previewClose(id).catch(() => {});
    setBuffers((prev) => prev.filter((b) => b.id !== id));
    unreadTabs.clear(id);
  }

  // Returns the path the copy was written to, which the caller opens as a note
  // of its own. The note the copy came from is left exactly where it is.
  async function saveCopy(id: string, content: string): Promise<string> {
    return api.saveNoteCopy(id, content);
  }

  // A discard drops what a failed save left in the unsaved-text journal, or the
  // next launch would bring back text the person was told is gone. When that
  // fails the close still goes ahead, and the next launch writes the text
  // beside the file.
  async function dropJournaledText(ids: string[]): Promise<void> {
    try {
      await api.discardUnsavedText(ids);
    } catch {
      logFailure("discarded text could not be dropped from the unsaved-text journal");
    }
  }

  async function closeBuffer(id: string, options: CloseOptions = {}): Promise<CloseOutcome> {
    // Closing a tab whose text did not reach disk would destroy it: the tab is
    // the only place that text still exists. Report the failure instead so the
    // caller can offer the choice, and keep the tab open until it does.
    if (options.discard) {
      cancelAutosave(id);
      await dropJournaledText([id]);
    } else {
      const flushed = await flushAutosave(id);
      if (!flushed.ok) return { closed: false, failures: flushed.failures };
    }
    await api.closeBuffer(id);
    // The preview pane is a window-lifetime singleton (never unmounted, to
    // avoid the writ-preview:// iframe teardown freeze), so it no longer evicts
    // the host render cache per buffer. Evict here, at the close that ends the
    // buffer's session.
    void api.previewClose(id).catch(() => {});
    const closedAt = new Date().toISOString();
    setBuffers((prev) =>
      prev.map((b) =>
        b.id === id ? { ...b, status: "history" as const, closed_at: closedAt } : b,
      ),
    );
    unreadTabs.clear(id);
    return { closed: true, failures: [] };
  }

  async function closeBuffers(ids: string[], options: CloseOptions = {}): Promise<BulkCloseOutcome> {
    if (ids.length === 0) return { closedIds: [], failedIds: [], failures: [] };

    let saved = ids;
    let failedIds: string[] = [];
    let failures: SaveFailure[] = [];
    if (options.discard) {
      for (const id of ids) cancelAutosave(id);
      await dropJournaledText(ids);
    } else {
      const flushed = await Promise.all(
        ids.map(async (id) => ({ id, result: await flushAutosave(id) })),
      );
      saved = flushed.filter((f) => f.result.ok).map((f) => f.id);
      failedIds = flushed.filter((f) => !f.result.ok).map((f) => f.id);
      failures = flushed.flatMap((f) => f.result.failures);
    }
    if (saved.length === 0) return { closedIds: [], failedIds, failures };
    await api.closeBuffers(saved);
    for (const id of saved) void api.previewClose(id).catch(() => {});
    const closedAt = new Date().toISOString();
    const closedIds = new Set(saved);
    setBuffers((prev) =>
      prev.map((b) =>
        closedIds.has(b.id)
          ? { ...b, status: "history" as const, closed_at: closedAt }
          : b,
      ),
    );
    // Only once the tabs have left the active set: the editor reads the tab in
    // front as its unread state clears, and Close All Tabs does not move the
    // selection first, so a tab cleared before it left was read as it closed.
    for (const id of saved) unreadTabs.clear(id);
    return { closedIds: saved, failedIds, failures };
  }

  // Reopening a closed file reads it, so it answers as an open does: a file
  // that grew past the size Writ opens without asking while it was closed is
  // asked about first, and one past the size Writ opens stays closed.
  async function restoreBuffer(id: string): Promise<OpenOutcome> {
    const closed = buffers().find((b) => b.id === id);
    const path = closed?.source_path ?? closed?.title ?? id;
    let result: FileOpenResult;
    try {
      result = await api.restoreBuffer(id);
    } catch (error) {
      return { kind: "refused", path, reason: openFailureReason(error) };
    }
    const outcome =
      result.mode.kind === "LargeFileConfirm"
        ? await openAfterAsking(path, result.size_bytes, () => api.restoreBufferConfirmed(id))
        : registerOpenResult(result, path);
    if (outcome.kind === "opened") await load();
    return outcome;
  }

  // One answer per unread tab at a time: a tab shown again while its question
  // is up waits for that answer rather than asking twice.
  const unreadOpens = new Map<string, Promise<OpenOutcome>>();

  // A tab restored unread at launch answers as reopening a closed file does:
  // its file is measured again, one over the size Writ opens without asking is
  // asked about, and the read waits for Open. Cancel leaves the tab unread,
  // and a refusal stays in the tab, which says why.
  function openUnreadTab(id: string): Promise<OpenOutcome> {
    const pending = unreadOpens.get(id);
    if (pending) return pending;
    const answer = answerUnreadTab(id).finally(() => unreadOpens.delete(id));
    unreadOpens.set(id, answer);
    return answer;
  }

  async function answerUnreadTab(id: string): Promise<OpenOutcome> {
    const row = buffers().find((b) => b.id === id);
    const path = row?.source_path ?? row?.title ?? id;
    let result: FileOpenResult;
    try {
      result = await api.restoreBuffer(id);
    } catch (error) {
      const reason = openFailureReason(error);
      unreadTabs.refuse(id, reason);
      return { kind: "refused", path, reason };
    }
    let outcome: OpenOutcome;
    if (result.mode.kind === "LargeFileConfirm") {
      unreadTabs.wait(id, result.size_bytes);
      outcome = await openAfterAsking(path, result.size_bytes, () =>
        api.restoreBufferConfirmed(id),
      );
    } else {
      outcome = registerOpenResult(result, path);
    }
    if (outcome.kind === "opened") unreadTabs.clear(id);
    else if (outcome.kind === "refused") unreadTabs.refuse(id, outcome.reason);
    return outcome;
  }

  async function deleteFromHistory(id: string): Promise<void> {
    await api.deleteBuffer(id);
    setBuffers((prev) => prev.filter((b) => b.id !== id));
  }

  async function clearAllHistory(): Promise<void> {
    await api.clearHistory();
    setBuffers((prev) => prev.filter((b) => b.status !== "history"));
  }

  // Renaming a note renames its file, so the row that comes back is the only
  // honest source of the new title and path: the name on disk is sanitised and
  // need not be the string that was typed.
  async function renameBuffer(id: string, title: string): Promise<void> {
    const doc = await api.renameNote(id, title);
    setBuffers((prev) => prev.map((b) => (b.id === doc.id ? doc : b)));
  }

  // The language chosen for a file with Change Language, or null to let its
  // name decide again. The choice lives on the file's row, so it outlasts the
  // tab; the open editor follows the row that comes back.
  async function setLanguage(id: string, language: string | null): Promise<void> {
    const doc = await api.setBufferLanguage(id, language);
    setBuffers((prev) => prev.map((b) => (b.id === doc.id ? doc : b)));
  }

  // `path` is what the caller asked to open; a result that names no file of
  // its own is answered under it.
  function registerOpenResult(result: FileOpenResult, path?: string): OpenOutcome {
    const asked = path ?? result.doc?.source_path ?? "";
    switch (result.mode.kind) {
      case "NotDownloaded":
        return {
          kind: "not-downloaded",
          path: result.mode.path,
          provider: result.mode.provider,
        };
      case "Refused":
        return {
          kind: "refused",
          path: asked,
          reason: refusalReason(result.mode.reason, result.size_bytes),
        };
    }
    const doc = result.doc;
    if (!doc) return { kind: "refused", path: asked, reason: openFailureReason(null) };
    const existed = activeTabs().some((b) => b.id === doc.id);
    openModes.set(doc.id, result.mode);
    setBuffers((prev) => {
      if (prev.find((b) => b.id === doc.id)) {
        return prev.map((b) => (b.id === doc.id ? doc : b));
      }
      return [...prev, doc];
    });
    return { kind: "opened", doc, existed, mode: result.mode };
  }

  // A file over the size Writ opens without asking: the size is stated before
  // anything reads it, and the progress runs from Open until the editor shows
  // the text (the tab store hands it on, or ends it when no tab shows it).
  async function openAfterAsking(
    path: string,
    sizeBytes: number,
    openConfirmed: () => Promise<FileOpenResult> = () => api.openFileConfirmed(path),
  ): Promise<OpenOutcome> {
    const open = await largeFileOpen.ask(basename(path), sizeBytes);
    if (!open) return { kind: "declined", path };
    await largeFileOpen.shown();
    let result: FileOpenResult;
    try {
      result = await openConfirmed();
    } catch (error) {
      largeFileOpen.finish();
      return { kind: "refused", path, reason: openFailureReason(error) };
    }
    const outcome = registerOpenResult(result, path);
    if (outcome.kind === "opened") largeFileOpen.handOff(outcome.doc.id);
    else largeFileOpen.finish();
    return outcome;
  }

  async function openFile(path: string): Promise<OpenOutcome> {
    const existing = activeTabs().find((b) => b.source_path === path);
    if (existing) {
      // A tab restored unread at launch has not been opened yet, so an open
      // of its path is its open, with the question or refusal that goes with it.
      if (unreadTabs.stateOf(existing.id)) return openUnreadTab(existing.id);
      const mode = openModes.get(existing.id) ?? modeOfRow(existing);
      return { kind: "opened", doc: existing, existed: true, mode };
    }

    let result: FileOpenResult;
    try {
      result = await api.openFile(path);
    } catch (error) {
      return { kind: "refused", path, reason: openFailureReason(error) };
    }
    if (result.mode.kind === "LargeFileConfirm") {
      return openAfterAsking(path, result.size_bytes);
    }
    const unread = unreadTabAnswered(result);
    if (unread !== null) return openUnreadTab(unread);
    return registerOpenResult(result, path);
  }

  // A path that spells an unread tab's file another way (a link, a case
  // variant in a link) is not the tab's path here, so Rust matches it and
  // answers with the tab's row, having read nothing. That tab's own open
  // answers: it measures the file, and asks or refuses.
  function unreadTabAnswered(result: FileOpenResult): string | null {
    const id = result.doc?.id;
    return id !== undefined && unreadTabs.stateOf(id) !== null ? id : null;
  }

  async function showOpenFileDialog(): Promise<string | null> {
    return api.showOpenFileDialog();
  }

  // A tab restored unread at launch is read only once its open answers: a
  // read before that loads a file nobody said Open to, or one past the size
  // Writ opens. Rust measures a file before it reads it, so one that grew
  // past that size since its open comes back as the refusal's code: the tab
  // is unread from then on, refused, and its pane says why.
  async function readContent(id: string): Promise<string> {
    if (unreadTabs.stateOf(id)) throw new UnreadTabReadError(id);
    try {
      return await api.readBufferContent(id);
    } catch (error) {
      if (leadingCodeOf(error, [ERR_OPEN_TOO_LARGE]) === null) throw error;
      // A size that could not be learned is worded as the limit itself, which
      // reads "over the limit".
      const size = (await recordedSizeOf(id)) ?? MAX_OPEN_BYTES;
      unreadTabs.takeRefused(id, tooLargeReason(size));
      throw new UnreadTabReadError(id);
    }
  }

  // The size Rust recorded on the row when it declined to read the file. The
  // registry's copy of the row takes it too, since the tab's tier is read off
  // its row.
  async function recordedSizeOf(id: string): Promise<number | null> {
    try {
      const doc = await api.getBuffer(id);
      setBuffers((prev) => prev.map((b) => (b.id === doc.id ? doc : b)));
      return doc.size_bytes;
    } catch {
      return null;
    }
  }

  /**
   * Re-reads one note's row and patches that entry.
   *
   * What a tab needs after its file moved: the row already names the new path
   * and the name it now has, and this is how the tab bar and the save path
   * catch up. One entry, never `load()` — reloading every buffer re-creates
   * the always-mounted preview iframe and freezes the macOS webview (#127).
   */
  async function refreshBuffer(id: string): Promise<void> {
    const doc = await api.getBuffer(id);
    setBuffers((prev) => prev.map((b) => (b.id === doc.id ? doc : b)));
  }

  return {
    buffers,
    activeTabs,
    historyList,
    load,
    loadSession,
    createBuffer,
    newNote,
    adoptDocument,
    todaysNote,
    deleteNote,
    saveCopy,
    closeBuffer,
    closeBuffers,
    restoreBuffer,
    openUnreadTab,
    deleteFromHistory,
    clearAllHistory,
    renameBuffer,
    setLanguage,
    openFile,
    registerOpenResult,
    showOpenFileDialog,
    readContent,
    refreshBuffer,
  };
}

export const bufferRegistry = createRoot(createBufferRegistry);
export type BufferRegistry = ReturnType<typeof createBufferRegistry>;
