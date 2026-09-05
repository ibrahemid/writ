import { isRetryableSaveError } from "../lib/save-error";
import { logFailure } from "../lib/log";
import { recordUnsavedNotes, saveBufferContent } from "./tauri";

// `generation` is the failed write's own generation, not the note's current
// one, so a listener can tell a refusal about a write it still cares about from
// one about a write that has since been superseded ([`currentSaveGeneration`]).
type AutosaveErrorListener = (
  bufferId: string,
  error: unknown,
  generation: number,
) => void;
// `diskHash` is the digest of what the note's file holds now, or null when the
// note had nothing in it and no file yet to write it to.
/**
 * `viaWriteBack` says the write went through a writer of its own rather than
 * the ordinary save. Only the put-a-deleted-file-back command does, and only
 * that write proves there is a file at the note's path now.
 */
type AutosaveSuccessListener = (
  bufferId: string,
  diskHash: string | null,
  viaWriteBack: boolean,
) => void;
type AutosaveStartListener = (bufferId: string) => void;

// Content may be a string or a lazy getter. A getter is materialized only when
// the save actually runs (timer fire or flush), so a large-buffer edit burst
// never forces a full `doc.toString()` on every keystroke just to feed the
// debounce, and flush stays correct because the getter reads the live document
// at flush time rather than a value captured keystrokes earlier (ADR-020).
export type ContentSource = string | (() => string);

export interface SaveFailure {
  bufferId: string;
  error: unknown;
}

// A save reports its outcome instead of rejecting: every caller awaits it on a
// path (tab close, window close, search) that must keep running when a write
// fails, and the one thing they all need is whether the text reached disk.
export interface SaveResult {
  ok: boolean;
  failures: SaveFailure[];
}

const SAVE_OK: SaveResult = { ok: true, failures: [] };

function saveFailed(bufferId: string, error: unknown): SaveResult {
  return { ok: false, failures: [{ bufferId, error }] };
}

function mergeResults(results: SaveResult[]): SaveResult {
  const failures = results.flatMap((r) => r.failures);
  return { ok: failures.length === 0, failures };
}

// Queued text carries the generation it was queued at. A failed write may put
// its text back only when no newer text has arrived for that buffer since; an
// empty queue is not proof of that, because a second write for the same buffer
// empties it too.
interface QueuedContent {
  source: ContentSource;
  generation: number;
  /**
   * The command this text is written through, when it is not the ordinary
   * save. A note whose file was deleted goes back through `restoreNoteFile`,
   * which the ordinary save is refused by, and it carries the writer with it
   * so a retry of the failed write is the same write and not a save the
   * backend will refuse again.
   */
  writer?: SaveWriter;
}

/** Writes a note's text and reports what its file holds afterwards. */
export type SaveWriter = (bufferId: string, content: string) => Promise<string | null>;

interface InFlightWrite {
  promise: Promise<SaveResult>;
  generation: number;
}

// A burst of sub-second pauses (paste, dictation, a macro) fires the idle
// debounce over and over, and every fire is a whole-note rewrite. The cap
// bounds that to one write per note per second. Only the scheduled path is
// capped: a flush and an explicit save are the deterministic "it is on disk"
// paths, and deferring those would lose text the process is about to stop
// being able to write.
const MIN_WRITE_INTERVAL_MS = 1000;

const timers = new Map<string, ReturnType<typeof setTimeout>>();
const lastWriteAt = new Map<string, number>();
const pendingContent = new Map<string, QueuedContent>();
const generations = new Map<string, number>();
const inFlight = new Map<string, InFlightWrite>();
const errorListeners = new Set<AutosaveErrorListener>();
const successListeners = new Set<AutosaveSuccessListener>();
const startListeners = new Set<AutosaveStartListener>();
// The text of the last write that failed, per note, so the quit path can keep
// it when the file could not. Kept beside the queue rather than in it because
// a guard-stopped write deliberately leaves the queue empty (writing the same
// text again is stopped the same way), and that text still has to reach the
// recovery snapshot.
const lastFailedContent = new Map<string, string>();

export function onAutosaveError(listener: AutosaveErrorListener): () => void {
  errorListeners.add(listener);
  return () => {
    errorListeners.delete(listener);
  };
}

export function onAutosaveSuccess(listener: AutosaveSuccessListener): () => void {
  successListeners.add(listener);
  return () => {
    successListeners.delete(listener);
  };
}

/** Fires as a write leaves for the backend, before its outcome is known. */
export function onAutosaveStart(listener: AutosaveStartListener): () => void {
  startListeners.add(listener);
  return () => {
    startListeners.delete(listener);
  };
}

/**
 * Text a note is keeping that no write may take.
 *
 * A note whose file is asking a question, or whose file is gone, writes
 * nothing until that ends ([`editorStore.savesAreHeld`]), so its typing has no
 * queue entry and no failed write to sit in, and closing the tab or quitting
 * would find nothing to keep. Beside the queue rather than in it: anything in
 * the queue is written by the next flush, which is the write the hold exists
 * to stop.
 *
 * Strings, never getters. A getter is bound to the editor's single view, and
 * a tab switch destroys that view and builds the next note's in its place, so
 * a getter left here would read the incoming note's document under the held
 * note's name. The queue can hold a getter because every load flushes it while
 * the view it was made against is still alive; a hold outlives exactly that.
 */
const heldContent = new Map<string, string>();

/**
 * Keeps a note's newest text without scheduling anything to write it.
 *
 * Read by the recovery handover ([`peekUnsavedContent`],
 * [`collectUnsavedContent`]) and by nothing that writes. Released when the
 * hold ends, because every way it can end has already dealt with the text.
 *
 * `content` is a string on purpose; see [`heldContent`].
 */
export function holdUnsavedContent(bufferId: string, content: string) {
  heldContent.set(bufferId, content);
}

/** Drops what [`holdUnsavedContent`] was keeping for a note that may write again. */
export function releaseUnsavedContent(bufferId: string) {
  heldContent.delete(bufferId);
}

/**
 * What a hold is keeping for `bufferId`, and nothing else.
 *
 * [`peekUnsavedContent`] falls through to a failed write's text, which for a
 * note that has just been answered is the version answered against. A caller
 * asking what was typed during the answer wants the hold alone.
 */
export function peekHeldContent(bufferId: string): string | undefined {
  return heldContent.get(bufferId);
}

/**
 * The newest text for `bufferId` that is not known to be on disk: whatever is
 * queued or held, else the text of the write that failed.
 *
 * `undefined` when the note has nothing outstanding. Materializes a queued
 * getter, so a caller reads the live document at the moment it asks.
 */
export function peekUnsavedContent(bufferId: string): string | undefined {
  const source = pendingContent.get(bufferId)?.source ?? heldContent.get(bufferId);
  if (source !== undefined) {
    try {
      return typeof source === "function" ? source() : source;
    } catch {
      // The live document is gone. Fall through to the failed text, which is
      // a plain string and outlives the view.
    }
  }
  return lastFailedContent.get(bufferId);
}

/**
 * Every note holding text that is not known to be on disk, with that text.
 *
 * The queue is not enough on its own: a write stopped by the guard empties the
 * queue on purpose, because writing the same text again is stopped the same
 * way, so the note whose failure is still on screen is exactly the one a
 * queue-only walk would miss. Nor is a note that is waiting to be asked about,
 * which never queued anything at all.
 */
export function collectUnsavedContent(): Array<{ id: string; content: string }> {
  const ids = new Set([
    ...pendingContent.keys(),
    ...heldContent.keys(),
    ...lastFailedContent.keys(),
  ]);
  const notes: Array<{ id: string; content: string }> = [];
  for (const id of ids) {
    const content = peekUnsavedContent(id);
    if (content !== undefined) notes.push({ id, content });
  }
  return notes;
}

/**
 * The generation a write issued for `bufferId` right now would carry.
 *
 * Every write the note has already issued carries this or less, and every
 * write issued after it carries more, because queueing and cancelling both
 * bump it. That is what lets a caller draw a line at a moment in time and act
 * on which side of it a write was on. Zero for a note that has never written.
 */
export function currentSaveGeneration(bufferId: string): number {
  return generations.get(bufferId) ?? 0;
}

function bumpGeneration(bufferId: string): number {
  const next = (generations.get(bufferId) ?? 0) + 1;
  generations.set(bufferId, next);
  return next;
}

function clearTimer(bufferId: string) {
  const existing = timers.get(bufferId);
  if (existing) {
    clearTimeout(existing);
    timers.delete(bufferId);
  }
}

function queueContent(bufferId: string, content: ContentSource, writer?: SaveWriter) {
  // The note is writing again, so the queue is the newer record of its text
  // and anything held for it is the older one.
  heldContent.delete(bufferId);
  pendingContent.set(bufferId, {
    source: content,
    generation: bumpGeneration(bufferId),
    writer,
  });
}

export function debouncedSave(bufferId: string, content: ContentSource, delayMs: number = 1000) {
  clearTimer(bufferId);
  queueContent(bufferId, content);

  armTimer(bufferId, delayMs);
}

function armTimer(bufferId: string, delayMs: number) {
  const timer = setTimeout(() => {
    timers.delete(bufferId);
    runScheduledSave(bufferId);
  }, delayMs);

  timers.set(bufferId, timer);
}

// A write that would land inside the cap is re-armed for the remainder of the
// window, never dropped: the text is still the newest the user typed, and a
// dropped write is the one failure autosave may not have.
function runScheduledSave(bufferId: string) {
  const last = lastWriteAt.get(bufferId);
  const waited = last === undefined ? MIN_WRITE_INTERVAL_MS : Date.now() - last;
  if (waited < MIN_WRITE_INTERVAL_MS) {
    armTimer(bufferId, MIN_WRITE_INTERVAL_MS - waited);
    return;
  }
  void runPendingSave(bufferId);
}

export function hasPendingAutosave(bufferId: string): boolean {
  return pendingContent.has(bufferId) || timers.has(bufferId) || inFlight.has(bufferId);
}

/**
 * Hands a closing note's unwritten text to the recovery snapshot, then drops
 * the record of it.
 *
 * A save the guard refused does not go back on the queue — writing the same
 * text into the same refusal is stopped the same way — so closing that tab
 * finds nothing to flush and closes without a word, while the text is still
 * only in this module. Left there it is lost when the process goes.
 *
 * What happens to it after the handover: the next quit writes it into the
 * shutdown snapshot, and the launch after that writes it back to the note's
 * file through the guarded path, which leaves a dated copy beside the file
 * rather than over it if the file has moved on
 * (`BufferStore::restore_recovered_content`). The note stays closed; the text
 * comes back as a file, and the toast counts it.
 *
 * The record is kept when the handover fails: text nobody can read again is a
 * worse outcome than a note restored twice.
 */
export async function keepUnsavedForRecovery(bufferId: string): Promise<void> {
  const content = peekUnsavedContent(bufferId);
  if (content !== undefined) {
    try {
      await recordUnsavedNotes([{ id: bufferId, content }]);
    } catch {
      logFailure("the text of a closed note could not be kept");
      return;
    }
  }
  cancelAutosave(bufferId);
}

export function cancelAutosave(bufferId: string) {
  clearTimer(bufferId);
  pendingContent.delete(bufferId);
  heldContent.delete(bufferId);
  lastWriteAt.delete(bufferId);
  lastFailedContent.delete(bufferId);
  // Retire the current generation so a write already in flight cannot put the
  // discarded text back on the queue when it fails.
  bumpGeneration(bufferId);
}

// Drops every scheduled save, queued edit and write-rate record at once.
// Autosave is a process-wide singleton (Writ is single-window), so a caller
// that drives the module in isolation — a test case, or a teardown — has no
// other way back to the start state; `cancelAutosave` is the per-note sibling.
export function resetAutosave() {
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
  pendingContent.clear();
  heldContent.clear();
  lastWriteAt.clear();
  generations.clear();
  inFlight.clear();
  lastFailedContent.clear();
}

export async function flushAutosave(bufferId?: string): Promise<SaveResult> {
  if (bufferId !== undefined) {
    clearTimer(bufferId);
    return runPendingSave(bufferId);
  }

  const ids = new Set<string>([
    ...timers.keys(),
    ...pendingContent.keys(),
    ...inFlight.keys(),
  ]);
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
  const results = await Promise.all(Array.from(ids, (id) => runPendingSave(id)));
  return mergeResults(results);
}

// Writes the buffer now whether or not an edit is pending: an explicit save is
// a deterministic "it is on disk" action, so it must not fall through to a
// no-op the way flushing an empty queue does. Reports through the same success
// and error listeners as an autosave.
export async function saveNow(
  bufferId: string,
  content: ContentSource,
  writer?: SaveWriter,
): Promise<SaveResult> {
  clearTimer(bufferId);
  queueContent(bufferId, content, writer);
  return runPendingSave(bufferId);
}

// One write per buffer at a time. Concurrent writes for one buffer race on the
// file and on the retry queue, and a caller that returns while a write is still
// outstanding lets the close path drop text that never reached disk. A caller
// arriving during a write waits for it, folds its outcome in, then writes
// whatever is queued behind it.
async function runPendingSave(bufferId: string): Promise<SaveResult> {
  const current = inFlight.get(bufferId);
  if (current === undefined) return startWrite(bufferId);

  const outcome = await current.promise;
  if (!pendingContent.has(bufferId) && !inFlight.has(bufferId)) return outcome;
  return mergeResults([outcome, await runPendingSave(bufferId)]);
}

async function startWrite(bufferId: string): Promise<SaveResult> {
  const queued = pendingContent.get(bufferId);
  if (queued === undefined) return SAVE_OK;
  pendingContent.delete(bufferId);

  const promise = writeQueued(bufferId, queued);
  inFlight.set(bufferId, { promise, generation: queued.generation });
  try {
    return await promise;
  } finally {
    if (inFlight.get(bufferId)?.promise === promise) inFlight.delete(bufferId);
  }
}

async function writeQueued(bufferId: string, queued: QueuedContent): Promise<SaveResult> {
  let content: string;
  try {
    content = typeof queued.source === "function" ? queued.source() : queued.source;
  } catch (error) {
    // The live document is gone (e.g. the view was torn down between schedule
    // and fire). Nothing to save; surface it like any other autosave failure.
    for (const listener of errorListeners) {
      listener(bufferId, error, queued.generation);
    }
    return saveFailed(bufferId, error);
  }

  lastWriteAt.set(bufferId, Date.now());
  for (const listener of startListeners) {
    listener(bufferId);
  }
  try {
    const diskHash = await (queued.writer ?? saveBufferContent)(bufferId, content);
    lastFailedContent.delete(bufferId);
    for (const listener of successListeners) {
      listener(bufferId, diskHash, queued.writer !== undefined);
    }
    return SAVE_OK;
  } catch (error) {
    // A failed write must not consume the text. It goes back on the queue as a
    // plain string (the getter's document may be gone by the next attempt) so
    // the next scheduled save or flush writes it again, but only while it is
    // still the newest text for this buffer.
    //
    // A write the guard stopped is the exception. Identical text is stopped
    // the same way, and a stopped save lands another dated copy beside the
    // note each time, so this text leaves the queue and the next keystroke —
    // which queues a new generation — is what writes again.
    const isNewest = generations.get(bufferId) === queued.generation;
    if (isNewest && isRetryableSaveError(error)) {
      pendingContent.set(bufferId, {
        source: content,
        generation: queued.generation,
        writer: queued.writer,
      });
    }
    // Only while it is still the newest text there is. A write whose
    // generation has been retired was superseded while it was out: by a
    // keystroke, by a tab closing, or by the person answering a question about
    // this file, which is the case that matters. Kept here it would fall
    // through [`peekUnsavedContent`] into the shutdown snapshot, and the next
    // launch would put the version they answered against back beside the note.
    if (isNewest) lastFailedContent.set(bufferId, content);
    for (const listener of errorListeners) {
      listener(bufferId, error, queued.generation);
    }
    return saveFailed(bufferId, error);
  }
}
