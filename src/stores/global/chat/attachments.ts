import { createSignal } from "solid-js";
import { chatAttachedSizes, type ChatAttachedSize } from "../../../services/tauri";
import { onEvent, type UnlistenFn } from "../../../services/events";
import { bufferRegistry } from "../buffer-registry";
import { linkStore } from "../link";
import { notesStore } from "../notes";
import { saveStatusStore } from "../save-status";
import { windowRegistry } from "../window-registry";
import { readableError } from "./errors";

/** Whether a chip's note can be read, and why not when it cannot. */
export type AttachmentState = "ok" | "unreadable";

/** A note the conversation carries, as the pane lists it. */
export interface Attachment {
  /** Absolute path, which is what the send names. */
  path: string;
  /** The file's name, which is what the row shows. */
  name: string;
  /** The note's size on disk. */
  bytes: number;
  /** What the folder calls this file, where one has been read: the
   * folder-relative slug for a note inside the notes folder, and the whole
   * absolute path for a file that is only reachable because a tab has it
   * open. Two paths with the same key are the same file. */
  key?: string;
  /** Added by the tab in front rather than by a person. One such chip at a
   * time: whichever tab comes forward next replaces it. */
  auto?: boolean;
  /** The folder whose chip carries this note, for a note attached by picking
   * a folder. The note still travels on its own, one per file. */
  viaFolder?: string;
  /** Whether the note could be read. Unknown until a read has been tried. */
  state?: AttachmentState;
  /** Why the note could not be read, in Rust's words. */
  reason?: string;
  /** The tab holding this note has unsaved text, so the send carries the
   * saved version rather than what is on screen (ADR-031 containment). */
  dirty?: boolean;
}

/** The tab the editor is showing, as the chip row follows it. */
export interface FrontTab {
  /** The tab's own id, which a removal is remembered against. */
  id: string;
  /** The note it holds, or null for a tab with no file yet. */
  note: Attachment | null;
}

/** What attaching a folder did, or the words it was refused in. */
export type AttachFolderResult = { ok: true; notes: number } | { ok: false; reason: string };

/** Why attaching the note in front was refused. */
export type AddOpenNoteRefusal = "unsaved" | "none";

export type AddOpenNoteResult =
  | { ok: true; path: string }
  | { ok: false; reason: AddOpenNoteRefusal };

/** How the conversation reads a note's size for the send dialog. */
export function totalBytes(attachments: readonly Attachment[]): number {
  return attachments.reduce((sum, note) => sum + note.bytes, 0);
}

/** The note's own name, which is what a chip shows. */
export function noteName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Whether a path is the whole of where a file is, rather than a key the notes
 * folder gives it. A file outside the folder is known by nothing else. */
export function isAbsolutePath(path: string): boolean {
  return path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path);
}

/** What a proposal's line reads: the key a note is filed under, and the file's
 * own name for a file outside the folder, whose whole path is in the title. */
export function proposalLabel(path: string): string {
  return isAbsolutePath(path) ? noteName(path) : path;
}

/** What a chip reads: the note's own name. Two notes of the same name are told
 * apart by the folder in the chip's title, which is what `key` carries. */
export function chipLabel(note: Attachment): string {
  return note.name.length > 0 ? note.name : noteName(note.path);
}

/** The whole of what the folder calls a note, for a line that has to tell two
 * notes of one name apart where no folder is shown beside it. */
export function noteKeyLabel(note: Attachment): string {
  return note.key ?? note.path;
}

/** What a folder's chip reads: the folder's own name, the slash saying it is
 * one. The folders above it are in the chip's title. */
export function folderChipLabel(folder: string): string {
  const parts = folder.split(/[\\/]/).filter((part) => part.length > 0);
  return `${parts[parts.length - 1] ?? folder}/`;
}

/** One row of the chip line: a note, or a folder with the notes it carries. */
export type ChipRow =
  | { kind: "note"; note: Attachment }
  | { kind: "folder"; folder: string; notes: Attachment[]; bytes: number };

/** The chip line, as the set reads: a note picked on its own is its own chip,
 * and the notes one folder brought are that folder's chip. The set stays flat,
 * so what the request carries is still one note per file. */
export function chipRows(attachments: readonly Attachment[]): ChipRow[] {
  const rows: ChipRow[] = [];
  const folders = new Map<string, Extract<ChipRow, { kind: "folder" }>>();
  for (const note of attachments) {
    const folder = note.viaFolder;
    if (folder === undefined) {
      rows.push({ kind: "note", note });
      continue;
    }
    const held = folders.get(folder);
    if (held === undefined) {
      const row: Extract<ChipRow, { kind: "folder" }> = {
        kind: "folder",
        folder,
        notes: [note],
        bytes: note.bytes,
      };
      folders.set(folder, row);
      rows.push(row);
      continue;
    }
    held.notes.push(note);
    held.bytes += note.bytes;
  }
  return rows;
}

/**
 * The chip set: what the next message carries, each note once, with the size
 * the file holds now.
 *
 * The notes belong to the conversation they were attached in (R5 design item
 * 7), so the facade empties the set when the conversation changes and the pane
 * offers the tab in front again.
 */
export function createAttachments() {
  const [attachments, setAttachments] = createSignal<Attachment[]>([]);
  // Counts the times the chip set was emptied for a conversation, which is
  // what the pane watches to offer the tab in front again.
  const [attachGeneration, setAttachGeneration] = createSignal(0);
  // Why the set as a whole cannot be sent, when no one note is the reason:
  // too many notes, or too many bytes between them. Rust says it, in the
  // sizes call that also reads each note.
  const [setRefusal, setSetRefusal] = createSignal<string | null>(null);
  // The tab the automatic chip is following, the tab whose automatic chip was
  // sent away, and the ticket the latest follow holds. Plain fields rather
  // than signals: the pane calls `followTab` from an effect, and an effect
  // that read these would re-run on its own writes.
  let autoTabId: string | null = null;
  let suppressedTabId: string | null = null;
  let followTicket = 0;
  // The note the last settled follow answered for, which is not always a chip:
  // a note already on the list by hand is answered by leaving it alone. Read
  // rather than the chip row, so a rebuilt tab list does not read disk again
  // for a tab that has already been answered.
  let followedPath: string | null = null;
  let notesSubscription: Promise<UnlistenFn> | null = null;

  /** Forgets which tab the automatic chip follows, without touching the set. */
  function forgetFollow() {
    autoTabId = null;
    suppressedTabId = null;
    followedPath = null;
  }

  /** Empties the chip set and asks the pane for the tab in front again.
   *
   * A conversation change takes the whole set with it, a removal included:
   * the next conversation starts from what the editor is showing. */
  function clearChips() {
    setAttachments([]);
    setSetRefusal(null);
    forgetFollow();
    setAttachGeneration((count) => count + 1);
  }

  /** Empties the set for a turn being edited, which brings its own notes. */
  function replaceWithNothing() {
    setAttachments([]);
    setSetRefusal(null);
  }

  /** Adds a note to what the request carries. Every chip is on the row the
   * pane shows, which is the whole of what leaves the machine (ADR-031 rule
   * 2.5): a person's pick, or the automatic chip of the tab in front. */
  function attach(note: Attachment) {
    setAttachments((held) =>
      held.some((other) => other.path === note.path) ? held : [...held, note],
    );
  }

  /** Takes a note back out of what the request carries. Sending the automatic
   * chip away is remembered against the tab it came from, so it does not
   * return while that tab is still the one in front. */
  function detach(path: string) {
    const held = attachments().find((note) => note.path === path);
    if (held?.auto === true) suppressedTabId = autoTabId;
    setAttachments((all) => all.filter((note) => note.path !== path));
    setSetRefusal(null);
  }

  function isAttached(path: string): boolean {
    return attachments().some((note) => note.path === path);
  }

  /** What the folder calls each of these notes, and how big each one is now,
   * keyed by the path that was asked about. A folder that refuses the read
   * answers nothing rather than failing the caller: a chip states what was
   * last read of it until a send reads it again. */
  async function readKeys(paths: string[]): Promise<Map<string, ChatAttachedSize>> {
    const rows = await chatAttachedSizes(paths).catch(() => []);
    return new Map(rows.map((row) => [row.path, row]));
  }

  /** Adds notes, each of them once, with the key the folder knows them by.
   *
   * A conversation file names a note by its folder-relative key and the pane
   * holds absolute paths, so one note reaches the list by two spellings. The
   * keys are read for the notes arriving and the held ones in one call, so a
   * note already on the list is recognised whichever spelling arrives. Every
   * writer that does not already know a note's key comes through here. */
  async function attachAll(notes: Attachment[]) {
    const held = attachments();
    const wanted = notes.filter(
      (note, index) =>
        !held.some((other) => other.path === note.path) &&
        notes.findIndex((other) => other.path === note.path) === index,
    );
    if (wanted.length === 0) return;
    const byPath = await readKeys([
      ...wanted.map((note) => note.path),
      ...held.map((note) => note.path),
    ]);
    const taken = new Set<string>();
    for (const note of held) {
      const key = byPath.get(note.path)?.key ?? note.key;
      if (key !== undefined) taken.add(key);
    }
    for (const note of wanted) {
      const found = byPath.get(note.path);
      const key = found?.key ?? note.key;
      if (key !== undefined) {
        if (taken.has(key)) continue;
        taken.add(key);
      }
      attach({ ...note, bytes: found?.bytes ?? note.bytes, key });
    }
  }

  /** Attaches every note one folder holds, the subfolders included.
   *
   * The folder is how the notes were chosen and how they are shown, not a
   * second kind of thing that travels: the request carries them one per file,
   * exactly as the folder's chip lists them (ADR-031 rule 2.5). The paths come
   * from the notes index, never from a directory walk.
   *
   * The limits are Rust's and so are the sentences they are refused in. The
   * sizes of the notes arriving and of the notes already held are read in one
   * call, which is the call that refuses a note over the per-note ceiling, a
   * conversation over the note count and a set over the byte total, so a
   * folder that would go over attaches nothing rather than as much of itself
   * as fits. */
  async function attachFolder(folder: string): Promise<AttachFolderResult> {
    const paths = await linkStore.notePathsInFolder(folder);
    if (paths.length === 0) return { ok: false, reason: "That folder holds no files." };
    const held = attachments();
    const wanted = paths.filter(
      (path, index) =>
        paths.indexOf(path) === index && !held.some((note) => note.path === path),
    );
    // Every note under it is already a chip, so the folder adds nothing and
    // refuses nothing.
    if (wanted.length === 0) return { ok: true, notes: 0 };
    let rows: ChatAttachedSize[];
    try {
      rows = await chatAttachedSizes([...wanted, ...held.map((note) => note.path)]);
    } catch (error) {
      return { ok: false, reason: readableError(error) };
    }
    const byPath = new Map(rows.map((row) => [row.path, row]));
    const taken = new Set(
      held
        .map((note) => byPath.get(note.path)?.key ?? note.key)
        .filter((key): key is string => key !== undefined),
    );
    let added = 0;
    for (const path of wanted) {
      const found = byPath.get(path);
      const key = found?.key;
      if (key !== undefined) {
        if (taken.has(key)) continue;
        taken.add(key);
      }
      attach({ path, name: noteName(path), bytes: found?.bytes ?? 0, key, viaFolder: folder });
      added += 1;
    }
    return { ok: true, notes: added };
  }

  /** Takes a folder's chip back out, and with it every note it carried: the
   * chip was one choice, so undoing it undoes the whole of it. */
  function detachFolder(folder: string) {
    setAttachments((all) => all.filter((note) => note.viaFolder !== folder));
    setSetRefusal(null);
  }

  /** Attaches a note the pane knows only a path for, reading its size from
   * disk so the chip and the send dialog state the same number. */
  async function attachByPath(path: string) {
    await attachAll([{ path, name: noteName(path), bytes: 0 }]);
  }

  /** Points the automatic chip at the tab the editor is showing.
   *
   * The note in front follows the editor: the tab that comes forward replaces
   * the chip the last one left, so the row states what the next message will
   * carry rather than what was in front when the pane opened. A chip a person
   * added is never the one replaced. A tab with no file gets no chip, and a
   * chip sent away stays away until another tab is focused, which is what
   * `detach` remembers.
   *
   * The read that resolves the note happens before the list is touched, and a
   * later call retires this one's ticket: holding Cmd+] cannot leave two
   * automatic chips or the chip of a tab that is no longer in front. */
  async function followTab(front: FrontTab | null): Promise<void> {
    const id = front?.id ?? null;
    const previous = autoTabId;
    if (id !== previous) {
      autoTabId = id;
      // Focusing another tab forgets a removal, the tab it was removed from
      // included: the offer is about what is in front, not about history.
      if (suppressedTabId !== null && suppressedTabId !== id) suppressedTabId = null;
    }
    const wanted = suppressedTabId !== null && suppressedTabId === id ? null : front?.note ?? null;
    if (id === previous && followedPath === (wanted?.path ?? null)) return;
    const ticket = (followTicket += 1);
    if (!wanted) {
      followedPath = null;
      setAttachments((held) => held.filter((note) => note.auto !== true));
      return;
    }
    const held = attachments().filter((note) => note.auto !== true);
    const byPath = await readKeys([wanted.path, ...held.map((note) => note.path)]);
    if (ticket !== followTicket) return;
    followedPath = wanted.path;
    const found = byPath.get(wanted.path);
    const key = found?.key ?? wanted.key;
    // One note is one chip however each spelling reached the list, so the
    // note in front is dropped where a person already picked it.
    const taken = new Set(
      held.map((note) => byPath.get(note.path)?.key ?? note.key).filter((one) => one !== undefined),
    );
    setAttachments((all) => {
      const kept = all.filter((note) => note.auto !== true);
      if (kept.some((note) => note.path === wanted.path)) return kept;
      if (key !== undefined && taken.has(key)) return kept;
      return [...kept, { ...wanted, bytes: found?.bytes ?? wanted.bytes, key, auto: true }];
    });
  }

  /** The tab in front, when it holds a note on disk. */
  function frontTab(): { id: string; path: string; bytes: number } | null {
    const activeId = windowRegistry.getActive()?.tabs.activeTabId();
    if (!activeId) return null;
    const doc = bufferRegistry.activeTabs().find((tab) => tab.id === activeId);
    if (!doc || doc.source_path === null) return null;
    return { id: doc.id, path: doc.source_path, bytes: doc.size_bytes };
  }

  /** Attaches the note in front, or says why it cannot be attached.
   *
   * A tab with no file has no path to name and nothing on disk to read, so it
   * is refused in words rather than silently skipped (R5 M2). */
  async function addOpenNote(): Promise<AddOpenNoteResult> {
    const activeId = windowRegistry.getActive()?.tabs.activeTabId();
    if (!activeId) return { ok: false, reason: "none" };
    const front = frontTab();
    if (!front) return { ok: false, reason: "unsaved" };
    await attachAll([{ path: front.path, name: noteName(front.path), bytes: front.bytes }]);
    return { ok: true, path: front.path };
  }

  /** Whether a tab holding this note has text the file does not.
   *
   * Never throws: the comparison lives in a window store reached through the
   * registry, and a pane rendered before one exists must still list its
   * chips. */
  function noteIsDirty(path: string): boolean {
    try {
      // An absolute key names one file, so it matches one tab or none: the
      // suffix is there for a folder-relative key and would otherwise be a
      // second way to land on the wrong tab.
      const whole = isAbsolutePath(path);
      const doc = bufferRegistry
        .activeTabs()
        .find(
          (tab) =>
            tab.source_path === path ||
            (!whole && tab.source_path !== null && tab.source_path.endsWith(`/${path}`)),
        );
      return doc ? saveStatusStore.stateOf(doc.id) === "dirty" : false;
    } catch {
      return false;
    }
  }

  /** The files open in a tab that the notes index does not already offer, for
   * the `@` list: a tab outside the notes folder is context because the user
   * opened it, and nothing else in the pane knows about it.
   *
   * With no notes root read yet every open file is offered, and the key each
   * one attaches under takes the duplicate back out. */
  function openTabCandidates(query: string, limit: number): { path: string; name: string }[] {
    const wanted = query.trim().toLowerCase();
    const found: { path: string; name: string }[] = [];
    try {
      const seen = new Set<string>();
      for (const tab of bufferRegistry.activeTabs()) {
        const path = tab.source_path;
        if (path === null || seen.has(path) || notesStore.contains(path)) continue;
        const name = noteName(path);
        if (wanted.length > 0 && !name.toLowerCase().includes(wanted)) continue;
        seen.add(path);
        found.push({ path, name });
        if (found.length === limit) break;
      }
    } catch {
      return [];
    }
    return found;
  }

  /** The attached notes with the sizes the files hold now, one note per note.
   *
   * A tab records a note's size when it reads it, and another program can
   * rewrite the file after that. The dialog asking to send it must state the
   * bytes the send will carry, so it asks disk rather than the tab. Two
   * spellings of one note are one line in that sentence and one path in the
   * request, which is why the send reads this list rather than the chips.
   *
   * It resolves whatever happens: a note that cannot be read is one chip
   * marked `unreadable` carrying Rust's reason, not a failed list that names
   * nothing (R5 M3). A set Rust refuses as a whole, with every note readable
   * on its own, is kept in `setRefusal` rather than lost to the one-by-one
   * reads that follow. */
  async function attachedOnDisk(): Promise<Attachment[]> {
    const held = attachments();
    if (held.length === 0) {
      setSetRefusal(null);
      return [];
    }
    let batchRefusal: string | null = null;
    const sizes = await chatAttachedSizes(held.map((note) => note.path)).catch(
      (error: unknown) => {
        batchRefusal = readableError(error);
        return null;
      },
    );
    // Keyed by the path that was asked about, which is the spelling these
    // attachments hold. The command's own folder-relative key names the same
    // note in a different shape and would miss every row.
    const byPath = new Map((sizes ?? []).map((note) => [note.path, note]));
    const reasons = new Map<string, string>();
    if (sizes === null) {
      // One unreadable note refuses the whole batch, so each is asked about on
      // its own: which note it is, and what Rust says about it.
      for (const note of held) {
        const one = await chatAttachedSizes([note.path]).catch((error: unknown) => {
          reasons.set(note.path, readableError(error));
          return [];
        });
        for (const row of one) byPath.set(row.path, row);
      }
    }
    setSetRefusal(sizes === null && reasons.size === 0 ? batchRefusal : null);
    const counted = new Set<string>();
    const shown: Attachment[] = [];
    for (const note of held) {
      const found = byPath.get(note.path);
      const key = found?.key ?? note.key;
      if (key !== undefined) {
        if (counted.has(key)) continue;
        counted.add(key);
      }
      const why = reasons.get(note.path);
      shown.push({
        ...note,
        bytes: found?.bytes ?? note.bytes,
        key,
        state: why === undefined ? "ok" : "unreadable",
        ...(why === undefined ? {} : { reason: why }),
        dirty: noteIsDirty(note.path),
      });
    }
    return shown;
  }

  /** Reads the chips' sizes again and writes them back, so a chip states the
   * note as it stands rather than as it stood when it was attached. */
  async function refreshAttachedSizes(): Promise<void> {
    if (attachments().length === 0) return;
    setAttachments(await attachedOnDisk());
  }

  /** Shows a list `attachedOnDisk()` already resolved, so the send reads disk
   * once and the chips still carry what that read found. */
  function setAttachedList(list: Attachment[]): void {
    setAttachments(list);
  }

  /** Follows the notes folder, so a chip states the size the next send will
   * carry rather than the size read when it was attached. Subscribed once; a
   * host with no event bus leaves the chips as they are rather than failing
   * the pane. */
  function watchNotes() {
    if (notesSubscription) return;
    try {
      notesSubscription = onEvent("notes:changed", (payload) => {
        if (!payload) return;
        const touched = attachments().some(
          (note) => note.path === payload.path || payload.path.endsWith(`/${note.path}`),
        );
        if (touched) void refreshAttachedSizes();
      });
      void notesSubscription.catch(() => undefined);
    } catch {
      notesSubscription = null;
    }
  }

  return {
    attachments,
    attachGeneration,
    setRefusal,
    forgetFollow,
    clearChips,
    replaceWithNothing,
    attach,
    detach,
    isAttached,
    attachAll,
    attachFolder,
    detachFolder,
    attachByPath,
    followTab,
    addOpenNote,
    openTabCandidates,
    attachedOnDisk,
    refreshAttachedSizes,
    setAttachedList,
    watchNotes,
  };
}

export type ChatAttachments = ReturnType<typeof createAttachments>;
