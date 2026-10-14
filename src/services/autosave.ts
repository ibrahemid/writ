import { isChangesRefusal, isRetryableSaveError } from "../lib/save-error";
import { logFailure } from "../lib/log";
import {
  discardUnsavedText,
  recordUnsavedNotes,
  saveBufferChanges,
  saveBufferContent,
} from "./tauri";

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
  write: LandedWrite,
) => void;
type AutosaveStartListener = (bufferId: string) => void;

/**
 * A document as it stood when it was handed over. Immutable, so holding one
 * costs nothing and reading it later reads what it held then, whatever has
 * happened to the editor since.
 */
export interface DocumentSnapshot {
  readonly length: number;
  // A function has a length and a toString too; this keeps a getter from
  // passing for a snapshot where only a snapshot may go ([`HeldContent`]).
  readonly lines: number;
  toString(): string;
}

// The text of a write: a string, a reader of the live document, or a snapshot.
// A reader or a snapshot is materialized only when the write runs, so typing
// never turns the document into one string just to feed the debounce.
export type ContentSource = string | (() => string) | DocumentSnapshot;

/** The text a content source stands for, read now. */
export function materializeContent(source: ContentSource): string {
  if (typeof source === "string") return source;
  if (typeof source === "function") return source();
  return source.toString();
}

/** What a write that landed carried. */
export interface LandedWrite {
  /** The text as it was handed over: the snapshot itself when it came as one. */
  source: string | DocumentSnapshot;
  /** The note's save generation the text was queued at ([`currentSaveGeneration`]). */
  generation: number;
}

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
  /**
   * Set for a note in the editor's large tiers: a snapshot is then sent as the
   * changes since the document the note's last write landed with, when one
   * is known ([`landedTexts`]), instead of as the whole text.
   */
  encodeChanges?: ChangeEncoder;
}

/** Writes a note's text and reports what its file holds afterwards. */
export type SaveWriter = (bufferId: string, content: string) => Promise<string | null>;

/** A change set and the length of the document it produces, as the save command takes them. */
export interface EncodedChanges {
  changes: unknown;
  length: number;
}

/**
 * The changes from `base` to `doc`, or null when they cannot be put that way
 * (either is not a document the encoder knows).
 */
export type ChangeEncoder = (base: DocumentSnapshot, doc: DocumentSnapshot) => EncodedChanges | null;

/** The document a note's last landed write carried, and the digest Rust answered it with. */
interface LandedText {
  doc: DocumentSnapshot;
  digest: string;
}

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

// A write that failed for a reason that passes (a file another program holds
// open, a share that dropped for a moment) is tried again on its own, sooner
// at first and then no more than every half minute, until it lands or newer
// text replaces it.
export const RETRY_FIRST_MS = 2000;
export const RETRY_MAX_MS = 30_000;
const retryAttempts = new Map<string, number>();

function retryDelay(attempt: number): number {
  return Math.min(RETRY_FIRST_MS * 2 ** attempt, RETRY_MAX_MS);
}

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
// unsaved-text journal.
const lastFailedContent = new Map<string, UnsavedText>();
// The text of each write still out, so a close that stops waiting on a write
// to a stalled share can still hand that text to the unsaved-text journal.
const inFlightContent = new Map<string, () => string>();
// Per note, the snapshot its last landed write carried and the digest of what
// that write put on the file. Only a snapshot is kept: the change set a
// large-tier save sends is made against exactly this document, and Rust
// applies it only while the file's text still has this digest. A write that
// lands carrying anything else, or through a writer of its own, drops it.
const landedTexts = new Map<string, LandedText>();

/**
 * Forgets the document a note's last write landed with, so its next write
 * sends the whole text. For a document the editor no longer holds: keeping
 * it here would keep it in memory.
 */
export function forgetLandedText(bufferId: string) {
  landedTexts.delete(bufferId);
}

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
 * What a hold keeps: a string, or a snapshot of the document. Never a getter
 * ([`heldContent`]).
 */
export type HeldContent = string | DocumentSnapshot;

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
 * Strings or snapshots, never getters. A getter is bound to the editor's
 * single view, and a tab switch destroys that view and builds the next note's
 * in its place, so a getter left here would read the incoming note's document
 * under the held note's name. The queue can hold a getter because every load
 * flushes it while the view it was made against is still alive; a hold
 * outlives exactly that. A snapshot is immutable and outlives the view, so
 * typing under a bar is held as the document stands and made a string only
 * when something reads it.
 */
const heldContent = new Map<string, HeldContent>();

/**
 * How long a hold waits after its newest text before handing it to the
 * unsaved-text journal, when the caller names no wait: the debounce
 * [`debouncedSave`] waits by default.
 */
const DEFAULT_KEEP_DELAY_MS = 1000;

// One per note whose held text is waiting to be journaled.
const keepTimers = new Map<string, ReturnType<typeof setTimeout>>();
// Notes whose held text has gone to the journal since their hold began, so
// the end of the hold knows there is an entry to answer for.
const keptHolds = new Set<string>();

/**
 * Keeps a note's newest text without scheduling anything to write it, and
 * hands it to the unsaved-text journal once `keepAfterMs` pass with no newer
 * text: the same debounce a save waits, so text a bar holds survives a crash
 * the way the text of a refused save does (ADR-044 §2).
 *
 * Read by the recovery handover ([`peekUnsavedContent`],
 * [`collectUnsavedContent`]) and by nothing that writes. Released when the
 * hold ends, because every way it can end has already dealt with the text.
 *
 * `content` is never a getter; see [`heldContent`].
 */
export function holdUnsavedContent(
  bufferId: string,
  content: HeldContent,
  keepAfterMs: number = DEFAULT_KEEP_DELAY_MS,
) {
  heldContent.set(bufferId, content);
  clearKeepTimer(bufferId);
  keepTimers.set(
    bufferId,
    setTimeout(() => {
      keepTimers.delete(bufferId);
      void keepHeldContent(bufferId);
    }, keepAfterMs),
  );
}

/**
 * Hands what the hold is keeping for `bufferId` to the journal. Marked kept
 * before the call leaves, so a hold that ends while it is out still answers
 * for the entry it is about to write.
 */
async function keepHeldContent(bufferId: string) {
  const held = heldContent.get(bufferId);
  if (held === undefined) return;
  keptHolds.add(bufferId);
  try {
    await recordUnsavedNotes([{ id: bufferId, content: materializeContent(held) }]);
  } catch {
    logFailure("the text of a note whose saves are held could not be kept");
  }
}

function clearKeepTimer(bufferId: string) {
  const existing = keepTimers.get(bufferId);
  if (existing !== undefined) {
    clearTimeout(existing);
    keepTimers.delete(bufferId);
  }
}

/**
 * Drops what [`holdUnsavedContent`] was keeping for a note that may write
 * again.
 *
 * Whatever ended the hold has dealt with its text: an answer or a write that
 * landed wrote it, or the person chose the file's text over it. So the entry
 * a keep wrote goes too, unless a write of that text follows this call
 * (`writeFollows`), which settles the entry itself when it lands or keeps
 * the text when it does not. Sent before any later write of the note, so the
 * journal sees the two in that order.
 */
export function releaseUnsavedContent(bufferId: string, writeFollows = false) {
  heldContent.delete(bufferId);
  clearKeepTimer(bufferId);
  if (keptHolds.delete(bufferId) && !writeFollows) void dropKeptContent(bufferId);
}

// The call leaves before this returns its promise, so it is on its way ahead
// of any write the caller starts next.
async function dropKeptContent(bufferId: string) {
  try {
    await discardUnsavedText([bufferId]);
  } catch {
    logFailure("the kept text of a note whose hold ended could not be dropped");
  }
}

/**
 * What a hold is keeping for `bufferId`, and nothing else.
 *
 * [`peekUnsavedContent`] falls through to a failed write's text, which for a
 * note that has just been answered is the version answered against. A caller
 * asking what was typed during the answer wants the hold alone.
 */
export function peekHeldContent(bufferId: string): string | undefined {
  const held = heldContent.get(bufferId);
  return held === undefined ? undefined : materializeContent(held);
}

/** Text not known to be on disk, with the write it belongs to. */
export interface UnsavedText {
  readonly content: string;
  /**
   * The save generation of the queued or failed write that carries the text
   * ([`currentSaveGeneration`]), or null for text a hold keeps. A failed
   * write's text stays here while newer writes go out.
   */
  readonly generation: number | null;
}

/**
 * The newest text for `bufferId` that is not known to be on disk: whatever is
 * queued or held, else the text of the write that failed.
 *
 * `undefined` when the note has nothing outstanding. Materializes a queued
 * getter, so a caller reads the live document at the moment it asks.
 */
export function peekUnsavedText(bufferId: string): UnsavedText | undefined {
  const queued = pendingContent.get(bufferId);
  const source = queued?.source ?? heldContent.get(bufferId);
  if (source !== undefined) {
    try {
      return { content: materializeContent(source), generation: queued?.generation ?? null };
    } catch {
      // The live document is gone. Fall through to the failed text, which is
      // a plain string and outlives the view.
    }
  }
  return lastFailedContent.get(bufferId);
}

/** [`peekUnsavedText`]'s text alone. */
export function peekUnsavedContent(bufferId: string): string | undefined {
  return peekUnsavedText(bufferId)?.content;
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
    ...inFlightContent.keys(),
  ]);
  const notes: Array<{ id: string; content: string }> = [];
  for (const id of ids) {
    const content = peekUnsavedContent(id) ?? inFlightContent.get(id)?.();
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

function queueContent(
  bufferId: string,
  content: ContentSource,
  writer?: SaveWriter,
  encodeChanges?: ChangeEncoder,
) {
  // The note is writing again, so the queue is the newer record of its text
  // and anything held for it is the older one. The write answers to the
  // journal from here: it clears the tab's entry when it lands and records
  // its text when it does not.
  heldContent.delete(bufferId);
  clearKeepTimer(bufferId);
  keptHolds.delete(bufferId);
  retryAttempts.delete(bufferId);
  pendingContent.set(bufferId, {
    source: content,
    generation: bumpGeneration(bufferId),
    writer,
    encodeChanges,
  });
}

export function debouncedSave(
  bufferId: string,
  content: ContentSource,
  delayMs: number = 1000,
  encodeChanges?: ChangeEncoder,
) {
  clearTimer(bufferId);
  queueContent(bufferId, content, undefined, encodeChanges);

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
 * Hands a closing note's unwritten text to the unsaved-text journal, then
 * drops the record of it.
 *
 * A save the guard refused does not go back on the queue — writing the same
 * text into the same refusal is stopped the same way — so closing that tab
 * finds nothing to flush and closes without a word, while the text is still
 * only in this module. Left there it is lost when the process goes.
 *
 * What happens to it after the handover: the journal keeps it from that
 * moment, and the next launch writes it back to the note's file through the
 * guarded path, which leaves a dated copy beside the file rather than over it
 * if the file has moved on (`BufferStore::restore_recovered_content`). The
 * note stays closed; the text comes back as a file, and the toast counts it.
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
  // The closed tab's entry is the journal's to keep now.
  keptHolds.delete(bufferId);
}

/**
 * Replaces a queued getter with the text it reads now, leaving it queued.
 *
 * For an editor view that is going away before its typing was written: the
 * getter would outlive the view it reads. The generation, the writer and the
 * timer stay, so this is the same write carrying a string. Queued strings,
 * an empty queue and a getter that can no longer read are left as they are.
 */
export function materializePendingSave(bufferId: string) {
  const queued = pendingContent.get(bufferId);
  if (queued === undefined || typeof queued.source !== "function") return;
  let text: string;
  try {
    text = queued.source();
  } catch {
    return;
  }
  pendingContent.set(bufferId, { ...queued, source: text });
}

export function cancelAutosave(bufferId: string) {
  clearTimer(bufferId);
  pendingContent.delete(bufferId);
  heldContent.delete(bufferId);
  // The record of a kept hold stays: a move from one hold into another
  // cancels here and holds again at once, and the entry is still there.
  clearKeepTimer(bufferId);
  lastWriteAt.delete(bufferId);
  lastFailedContent.delete(bufferId);
  inFlightContent.delete(bufferId);
  retryAttempts.delete(bufferId);
  landedTexts.delete(bufferId);
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
  for (const timer of keepTimers.values()) clearTimeout(timer);
  keepTimers.clear();
  keptHolds.clear();
  lastWriteAt.clear();
  generations.clear();
  inFlight.clear();
  inFlightContent.clear();
  retryAttempts.clear();
  lastFailedContent.clear();
  landedTexts.clear();
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
  encodeChanges?: ChangeEncoder,
): Promise<SaveResult> {
  clearTimer(bufferId);
  queueContent(bufferId, content, writer, encodeChanges);
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
    if (inFlight.get(bufferId)?.promise === promise) {
      inFlight.delete(bufferId);
      inFlightContent.delete(bufferId);
    }
  }
}

/**
 * The change set a queued snapshot can be sent as: made against the document
 * the note's last write landed with, for a note whose writes may go that way.
 */
function changesToSend(
  bufferId: string,
  queued: QueuedContent,
): (EncodedChanges & { base: string }) | null {
  if (queued.writer !== undefined || queued.encodeChanges === undefined) return null;
  if (typeof queued.source !== "object") return null;
  const landed = landedTexts.get(bufferId);
  if (landed === undefined) return null;
  try {
    const encoded = queued.encodeChanges(landed.doc, queued.source);
    return encoded === null ? null : { ...encoded, base: landed.digest };
  } catch {
    logFailure("the changes since the last save could not be worked out");
    return null;
  }
}

/**
 * Writes the queued text as a change set. A change set Rust will not apply
 * (the file moved on, or it does not fit) is the same write sent again
 * whole, so the person sees one save either way.
 */
async function sendChanges(
  bufferId: string,
  queued: QueuedContent,
  changes: EncodedChanges & { base: string },
  whole: () => string,
): Promise<string | null> {
  try {
    return await saveBufferChanges(bufferId, changes.base, changes.changes, changes.length);
  } catch (error) {
    if (!isChangesRefusal(error)) throw error;
    landedTexts.delete(bufferId);
  }
  return (queued.writer ?? saveBufferContent)(bufferId, whole());
}

async function writeQueued(bufferId: string, queued: QueuedContent): Promise<SaveResult> {
  // A snapshot that may go as a change set is turned into a string only if
  // the whole text has to go after all; anything else is read now, while the
  // reader it may be still reads the note it was queued for.
  let content: string | undefined;
  const changes = changesToSend(bufferId, queued);
  if (changes === null) {
    try {
      content = materializeContent(queued.source);
    } catch (error) {
      // The live document is gone (e.g. the view was torn down between schedule
      // and fire). Nothing to save; surface it like any other autosave failure.
      for (const listener of errorListeners) {
        listener(bufferId, error, queued.generation);
      }
      return saveFailed(bufferId, error);
    }
  }
  const whole = () => (content ??= materializeContent(queued.source));

  lastWriteAt.set(bufferId, Date.now());
  for (const listener of startListeners) {
    listener(bufferId);
  }
  inFlightContent.set(bufferId, whole);
  try {
    const diskHash = await (changes === null
      ? (queued.writer ?? saveBufferContent)(bufferId, whole())
      : sendChanges(bufferId, queued, changes, whole));
    lastFailedContent.delete(bufferId);
    retryAttempts.delete(bufferId);
    if (typeof queued.source === "object" && queued.writer === undefined && diskHash !== null) {
      landedTexts.set(bufferId, { doc: queued.source, digest: diskHash });
    } else {
      landedTexts.delete(bufferId);
    }
    const write: LandedWrite = {
      source: typeof queued.source === "object" ? queued.source : whole(),
      generation: queued.generation,
    };
    for (const listener of successListeners) {
      listener(bufferId, diskHash, queued.writer !== undefined, write);
    }
    return SAVE_OK;
  } catch (error) {
    // A failed write must not consume the text. It goes back on the queue as a
    // plain string, or as the snapshot it came as (a reader's document may be
    // gone by the next attempt), so the next scheduled save or flush writes it
    // again, but only while it is still the newest text for this buffer.
    //
    // A write the guard stopped is the exception. Identical text is stopped
    // the same way, and a stopped save lands another dated copy beside the
    // note each time, so this text leaves the queue and the next keystroke —
    // which queues a new generation — is what writes again.
    const text = whole();
    const isNewest = generations.get(bufferId) === queued.generation;
    if (isNewest && isRetryableSaveError(error)) {
      pendingContent.set(bufferId, {
        source: typeof queued.source === "object" ? queued.source : text,
        generation: queued.generation,
        writer: queued.writer,
        encodeChanges: queued.encodeChanges,
      });
      // A keystroke or flush in the meantime writes it sooner; this is for
      // the note nobody touches again.
      if (!timers.has(bufferId)) {
        const attempt = retryAttempts.get(bufferId) ?? 0;
        retryAttempts.set(bufferId, attempt + 1);
        armTimer(bufferId, retryDelay(attempt));
      }
    }
    // Only while it is still the newest text there is. A write whose
    // generation has been retired was superseded while it was out: by a
    // keystroke, by a tab closing, or by the person answering a question about
    // this file, which is the case that matters. Kept here it would fall
    // through [`peekUnsavedContent`] into the unsaved-text journal, and the
    // next launch would put the version they answered against back beside the
    // note.
    if (isNewest) lastFailedContent.set(bufferId, { content: text, generation: queued.generation });
    for (const listener of errorListeners) {
      listener(bufferId, error, queued.generation);
    }
    return saveFailed(bufferId, error);
  }
}
