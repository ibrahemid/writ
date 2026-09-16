import { createSignal, createMemo, createRoot } from "solid-js";
import {
  chatState,
  chatAttachedSizes,
  chatList,
  chatOpen,
  chatNew,
  chatRename,
  chatDelete,
  chatRenderReply,
  chatSend,
  chatStop,
  chatApplyProposal,
  chatDiscardProposal,
  type ChatDroppedProposal,
  type ChatEndpointState,
  type ChatAttachmentRef,
  type ChatConversation,
  type ChatConversationSummary,
  type ChatProposal,
  type ChatProposalOutcome,
  type ChatStoredTurn,
  type DiffHunk,
  type DiffLine,
  type RequestIdentity,
} from "../../services/tauri";
import { onEvent, type UnlistenFn } from "../../services/events";
import { aiConnectionStore } from "./ai-connection";
import { bufferRegistry } from "./buffer-registry";
import { configStore } from "./config";
import { saveStatusStore } from "./save-status";
import { windowRegistry } from "./window-registry";
import { showToast } from "../../components/Notifications/Toast";
import { writeClipboardText } from "../../services/clipboard";
import type { WritEvent } from "../../types/events";

export type {
  ChatAttachmentRef,
  ChatConversation,
  ChatConversationSummary,
  ChatDroppedProposal,
  ChatProposal,
  DiffHunk,
  DiffLine,
  RequestIdentity,
};

type ChatPayload = Extract<WritEvent, { kind: "ai:chat" }>["payload"];

export type ChatStatus = "idle" | "thinking" | "streaming" | "done" | "stopped" | "error";

/** How often a live reply is re-rendered while it streams (ADR-040 section 9). */
export const RENDER_THROTTLE_MS = 120;

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
  /** The note's folder-relative key, where one has been read. Two paths with
   * the same key are the same note. */
  key?: string;
  /** Added by the pane-open latch rather than by a person. One such chip at a
   * time: a later opening replaces it. */
  auto?: boolean;
  /** Whether the note could be read. Unknown until a read has been tried. */
  state?: AttachmentState;
  /** Why the note could not be read, in Rust's words. */
  reason?: string;
  /** The tab holding this note has unsaved text, so the send carries the
   * saved version rather than what is on screen (ADR-031 containment). */
  dirty?: boolean;
}

/** Why attaching the note in front was refused. */
export type AddOpenNoteRefusal = "unsaved" | "none";

export type AddOpenNoteResult =
  | { ok: true; path: string }
  | { ok: false; reason: AddOpenNoteRefusal };

/** One turn on screen, at the index the stored conversation holds it. */
export interface Message {
  /** Where this turn sits in the stored conversation, which is the number
   * `chat_apply_proposal` takes. */
  turn: number;
  role: "user" | "assistant";
  content: string;
  /** The reply as a rendered fragment, empty until it has been rendered. */
  html: string;
  attachments: ChatAttachmentRef[];
  proposals: ChatProposal[];
  /** Blocks the reply wrote that could not become a proposal. */
  dropped: ChatDroppedProposal[];
  /** The reply stopped at the model's token ceiling. */
  truncated: boolean;
  /** Which connection wrote this reply, where the frame said so. */
  identity: RequestIdentity | null;
}

/** One conversation's session state: the send in flight, what it has shown,
 * and what each of its turns has been rendered to.
 *
 * There is one of these per conversation rather than one for the pane, so a
 * reply keeps arriving into the conversation it belongs to while the pane
 * shows another (R4 section 3).
 *
 * `renderGeneration` and `renderTimer` are stable mutable holders: they are
 * created with the entry and carried unchanged through every replacement of
 * it, so a spread never leaves a timer nobody can cancel.
 */
interface Exchange {
  /** The send in flight, or empty when nothing is. */
  requestId: string;
  user: Message | null;
  reply: Message | null;
  status: ChatStatus;
  errorMessage: string;
  /** The typed failure's kind, for the recovery action. Empty for an untyped
   * frame. */
  errorKind: string;
  /** Which connection refused, where the failure named one. */
  errorIdentity: RequestIdentity | null;
  lastSend: { turn: number; text: string; paths: string[] } | null;
  htmlByTurn: Record<number, string>;
  renderGeneration: Map<number, number>;
  renderTimer: { handle: ReturnType<typeof setTimeout> | null };
}

function blankExchange(): Exchange {
  return {
    requestId: "",
    user: null,
    reply: null,
    status: "idle",
    errorMessage: "",
    errorKind: "",
    errorIdentity: null,
    lastSend: null,
    htmlByTurn: {},
    renderGeneration: new Map(),
    renderTimer: { handle: null },
  };
}

/** How the conversation reads a note's size for the send dialog. */
export function totalBytes(attachments: readonly Attachment[]): number {
  return attachments.reduce((sum, note) => sum + note.bytes, 0);
}

/** The note's own name, which is what a chip shows. */
export function noteName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** What a chip reads: the note's folder-relative key with the folders above
 * the last one elided. Two notes of the same name in different folders read
 * differently; the whole key is in `key`, for the row's title. */
export function chipLabel(note: Attachment): string {
  const key = note.key ?? note.path;
  const parts = key.split(/[\\/]/).filter((part) => part.length > 0);
  if (parts.length <= 2) return parts.join("/");
  return `…/${parts.slice(-2).join("/")}`;
}

/** Whether a rebuilt turn shows anything the last one did not. A reload hands
 * back turns that are equal in new arrays, so this reads the fields rather
 * than the references. */
function sameMessage(held: Message, next: Message): boolean {
  return (
    held.role === next.role &&
    held.content === next.content &&
    held.html === next.html &&
    held.truncated === next.truncated &&
    held.identity === next.identity &&
    sameLength(held.dropped, next.dropped) &&
    held.dropped.every(
      (drop, index) =>
        drop.named === next.dropped[index].named && drop.reason === next.dropped[index].reason,
    ) &&
    sameLength(held.attachments, next.attachments) &&
    held.attachments.every((note, index) => sameAttachment(note, next.attachments[index])) &&
    sameLength(held.proposals, next.proposals) &&
    held.proposals.every((offer, index) => sameProposal(offer, next.proposals[index]))
  );
}

function sameLength(held: readonly unknown[], next: readonly unknown[]): boolean {
  return held.length === next.length;
}

function sameAttachment(held: ChatAttachmentRef, next: ChatAttachmentRef): boolean {
  return held.path === next.path && held.bytes === next.bytes && held.hash === next.hash;
}

function sameProposal(held: ChatProposal, next: ChatProposal): boolean {
  return (
    held.path === next.path &&
    held.summary === next.summary &&
    held.before_hash === next.before_hash &&
    held.new_content === next.new_content &&
    held.status === next.status &&
    held.stale === next.stale &&
    sameLength(held.hunks, next.hunks) &&
    held.hunks.every((hunk, index) => sameHunk(hunk, next.hunks[index]))
  );
}

/** The lines are compared as well as the text they came from: `chat_open` reads
 * a pending proposal against the note as it stands, so an offer can come back
 * with the same text and a different diff. */
function sameHunk(held: DiffHunk, next: DiffHunk): boolean {
  return (
    held.before_start === next.before_start &&
    held.after_start === next.after_start &&
    sameLength(held.lines, next.lines) &&
    held.lines.every(
      (line, index) => line.kind === next.lines[index].kind && line.text === next.lines[index].text,
    )
  );
}

/** What the pane is waiting on before a message can be sent, for the composer
 * to say. It is a hint and not a gate: the blockers themselves are resolved by
 * the send preflight, which asks Rust. */
export type ReadinessState =
  | "ready"
  | "off"
  | "no_model"
  | "no_key"
  | "offline_local"
  | "model_unavailable"
  | "unconsented";

/** What pressing the readiness line does. */
export type ReadinessAction =
  | { kind: "settings"; section: "ai"; setting: string }
  | { kind: "check" };

export type Readiness =
  | { state: "ready" }
  | { state: Exclude<ReadinessState, "ready">; message: string; action: ReadinessAction };

function newRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

// Singleton state — Writ is single-window. One conversation is on screen at a
// time; the entries behind it are per conversation, because a reply keeps
// arriving into the conversation it was sent from.
function createChatStore() {
  const [conversations, setConversations] = createSignal<ChatConversationSummary[]>([]);
  const [current, setCurrent] = createSignal<ChatConversation | null>(null);
  const [exchanges, setExchanges] = createSignal<Record<string, Exchange>>({});
  const [attachments, setAttachments] = createSignal<Attachment[]>([]);
  // A failure that belongs to the pane rather than to any one conversation:
  // opening, renaming and deleting all end here, and none of them may write
  // over the status of an exchange that is still streaming.
  const [paneError, setPaneError] = createSignal("");
  const [draft, setDraft] = createSignal("");
  const [editing, setEditing] = createSignal<number | null>(null);
  const [refusals, setRefusals] = createSignal<Record<string, string>>({});
  // Counts the chat openings the pane has to attach the note in front for: the
  // pane latches on it, so one open attaches once however often it re-renders.
  const [attachGeneration, setAttachGeneration] = createSignal(0);

  // What each turn has been asked to render and what came back, keyed by
  // conversation and turn. A settle renders the finished reply and the reload
  // behind it walks the same turns, so without this one ending is two renders.
  // A null fragment is a render still in flight.
  const renderAsked = new Map<string, { markdown: string; html: string | null }>();
  // Proposals whose write is in flight, keyed `${turn}:${path}`. Applying is
  // one-shot: a second press while the first write runs does nothing.
  const applying = new Set<string>();
  // Held for as long as a send is between the draft and the status that says
  // the pane is busy. Retry takes it too, because it is the other way in.
  let sending = false;
  let notesSubscription: Promise<UnlistenFn> | null = null;

  function entryOf(id: string | undefined | null): Exchange | undefined {
    return id ? exchanges()[id] : undefined;
  }

  const currentEntry = (): Exchange | undefined => entryOf(current()?.id);

  /** Makes sure a conversation has session state, and answers it. */
  function ensureEntry(id: string): Exchange {
    const held = exchanges()[id];
    if (held) return held;
    const made = blankExchange();
    setExchanges((all) => ({ ...all, [id]: made }));
    return made;
  }

  /** Replaces part of one conversation's entry, leaving every other alone. */
  function patchEntry(id: string, change: Partial<Exchange>): void {
    setExchanges((all) => {
      const held = all[id];
      if (!held) return all;
      return { ...all, [id]: { ...held, ...change } };
    });
  }

  function dropEntry(id: string): void {
    const held = exchanges()[id];
    if (held) cancelScheduledRender(held);
    setExchanges((all) => {
      if (!(id in all)) return all;
      const next = { ...all };
      delete next[id];
      return next;
    });
    for (const key of [...renderAsked.keys()]) {
      if (key.startsWith(`${id}:`)) renderAsked.delete(key);
    }
  }

  function toMessage(turn: ChatStoredTurn, index: number, html: string): Message {
    return {
      turn: index,
      role: turn.role,
      content: turn.content,
      html,
      attachments: turn.attachments,
      proposals: turn.proposals,
      dropped: turn.dropped ?? [],
      truncated: turn.truncated ?? false,
      identity: null,
    };
  }

  // The turns as the pane last saw them, pending tail included, and which
  // conversation they belong to: turn 3 of one chat and turn 3 of another are
  // different turns, and reusing one object across the two would show one
  // conversation's text under the other's index.
  let lastShown: { id: string | null; list: Message[] } = { id: null, list: [] };

  /** The turns the file holds, with what each has been rendered to.
   *
   * A delta changes neither the conversation nor any settled turn's fragment,
   * so this returns the list it returned last: the settled half of a long
   * conversation is not rebuilt, and not re-compared, once per token. The
   * comparison is kept for the reload behind a settle, which hands back equal
   * turns in new objects. */
  const settled = createMemo<{
    source: ChatConversation | null;
    html: string[];
    list: Message[];
  }>((before) => {
    const conversation = current();
    const rendered = currentEntry()?.htmlByTurn ?? {};
    const turns = conversation ? conversation.turns : [];
    const html = turns.map((_turn, index) => rendered[index] ?? "");
    if (
      before &&
      before.source === conversation &&
      before.html.length === html.length &&
      before.html.every((text, index) => text === html[index])
    ) {
      return before;
    }
    const shown = turns.map((turn, index) => toMessage(turn, index, html[index]));
    const held =
      lastShown.id === (conversation?.id ?? null)
        ? new Map(lastShown.list.map((message) => [message.turn, message]))
        : new Map<number, Message>();
    const list = shown.map((message) => {
      const kept = held.get(message.turn);
      return kept && sameMessage(kept, message) ? kept : message;
    });
    return { source: conversation, html, list };
  });

  /** The turns of a send the file does not hold yet: the person's, and the
   * reply as far as it has arrived. This is the only part a delta rebuilds.
   *
   * A turn the file already holds is not repeated: the user turn is durable
   * from the moment `chat_send` is accepted, so a conversation reopened
   * mid-stream reads it from the file and only the reply is still pending. */
  const pending = createMemo<Message[]>(() => {
    const entry = currentEntry();
    if (!entry) return [];
    const held = current()?.turns.length ?? 0;
    const tail: Message[] = [];
    if (entry.user && held <= entry.user.turn) tail.push(entry.user);
    if (entry.reply && held <= entry.reply.turn) {
      tail.push({ ...entry.reply, html: entry.htmlByTurn[entry.reply.turn] ?? "" });
    }
    return tail;
  });

  /** The turns on screen, oldest first. A turn that says what it said keeps
   * its object, so its element is left alone and the copy button a person just
   * pressed is still the element that was pressed. */
  const messages = createMemo<Message[]>(() => {
    const shown = [...settled().list, ...pending()];
    lastShown = { id: current()?.id ?? null, list: shown };
    return shown;
  });

  /** Whether a reply is still arriving into that conversation, on screen or
   * not. */
  function isLive(id: string): boolean {
    const held = entryOf(id)?.status;
    return held === "thinking" || held === "streaming";
  }

  /** Every conversation with a reply still arriving. */
  function liveConversations(): string[] {
    return Object.keys(exchanges()).filter((id) => isLive(id));
  }

  function statusOf(id: string): ChatStatus {
    return entryOf(id)?.status ?? "idle";
  }

  function status(): ChatStatus {
    const held = currentEntry()?.status ?? "idle";
    if (held !== "idle") return held;
    return paneError() ? "error" : held;
  }

  function errorMessage(): string {
    return currentEntry()?.errorMessage || paneError();
  }

  /** The typed failure's kind, which is what a recovery action is chosen
   * from. Empty where the frame carried no typed error. */
  function errorKind(): string {
    return currentEntry()?.errorKind ?? "";
  }

  /** Which connection refused, so the line can name the model. */
  function errorIdentity(): RequestIdentity | null {
    return currentEntry()?.errorIdentity ?? null;
  }

  function isBusy(): boolean {
    const id = current()?.id;
    return id ? isLive(id) : false;
  }

  /** The send Retry would repeat, from this session or from the file.
   *
   * Nothing is live after a relaunch, and a conversation whose last turn is a
   * person's is exactly a send whose reply never arrived, so it is offered
   * again from the file rather than from session state nothing kept. */
  function retryable(): { turn: number; text: string; paths: string[] } | null {
    const entry = currentEntry();
    if (entry?.lastSend) return entry.lastSend;
    const turns = current()?.turns ?? [];
    const last = turns[turns.length - 1];
    if (!last || last.role !== "user") return null;
    return {
      turn: turns.length - 1,
      text: last.content,
      paths: last.attachments.map((note) => note.path),
    };
  }

  /** Whether the pane holds a message it could send again. */
  function canRetry(): boolean {
    return retryable() !== null;
  }

  /** Shows a failure that belongs to the pane and to no conversation. */
  function failWith(message: string) {
    setPaneError(message);
  }

  /** Clears the view without touching any conversation's entry or the chips. */
  function clearView() {
    setCurrent(null);
    setRefusals({});
    setPaneError("");
    setEditing(null);
    setDraft("");
  }

  /** Puts the pane back to holding nothing: no conversation on screen and no
   * session state behind one. The chips are left alone, because they are
   * cleared where a conversation changes (R5 design item 7). */
  function reset() {
    for (const id of Object.keys(exchanges())) dropEntry(id);
    clearView();
  }

  /** Adds a note to what the request carries. Attaching is a user's action and
   * nothing else adds one, so the list the pane shows is the whole of what
   * leaves the machine (ADR-031 rule 2.5). */
  function attach(note: Attachment) {
    setAttachments((held) =>
      held.some((other) => other.path === note.path) ? held : [...held, note],
    );
  }

  function detach(path: string) {
    setAttachments((held) => held.filter((note) => note.path !== path));
  }

  function isAttached(path: string): boolean {
    return attachments().some((note) => note.path === path);
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
    const sizes = await chatAttachedSizes([
      ...wanted.map((note) => note.path),
      ...held.map((note) => note.path),
    ]).catch(() => []);
    const byPath = new Map(sizes.map((note) => [note.path, note]));
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

  /** Attaches a note the pane knows only a path for, reading its size from
   * disk so the chip and the send dialog state the same number. */
  async function attachByPath(path: string) {
    await attachAll([{ path, name: noteName(path), bytes: 0 }]);
  }

  /** The note the pane opened over, as one replaceable chip.
   *
   * Opening the pane offers the note in front. Opening it again offers
   * whatever is in front then, in place of the last offer rather than beside
   * it: a pane toggle must not grow what the next message carries (R5 design
   * item 2). A chip a person added by hand is never the one replaced, and
   * removing the automatic chip leaves the slot empty until the next opening.
   */
  async function attachAuto(note: Attachment | null) {
    setAttachments((held) => held.filter((other) => other.auto !== true));
    if (!note) return;
    if (isAttached(note.path)) return;
    await attachAll([note]);
    // Marked by path rather than by what the list held before the read: a
    // note-changed frame can rebuild the list while that read is in flight.
    setAttachments((held) =>
      held.map((other) => (other.path === note.path ? { ...other, auto: true } : other)),
    );
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
      const doc = bufferRegistry
        .activeTabs()
        .find(
          (tab) =>
            tab.source_path === path ||
            (tab.source_path !== null && tab.source_path.endsWith(`/${path}`)),
        );
      return doc ? saveStatusStore.stateOf(doc.id) === "dirty" : false;
    } catch {
      return false;
    }
  }

  /** The models the connection's provider lists, for the picker.
   *
   * Read from `aiConnectionStore` and held nowhere else, so the pane and
   * Settings cannot offer different inventories (R2 design section 3). */
  function liveModels(): string[] {
    const catalog = aiConnectionStore.catalog();
    return catalog?.source === "live" ? catalog.models : [];
  }

  /** The chat's model: its own choice while that choice belongs to the
   * connection's provider, and the connection's model otherwise. */
  function chatModel(): string {
    const ai = configStore.config().ai;
    const override = ai.chat.model.trim();
    return override && ai.chat.model_provider === ai.provider ? override : ai.model;
  }

  /** What the connection still needs before a message can be sent.
   *
   * Only positive evidence blocks: a connection nobody has checked reads
   * ready, so a pane that has asked nothing does not accuse the person of a
   * missing key. */
  function readiness(): Readiness {
    const ai = configStore.config().ai;
    if (!ai.chat.enabled) {
      return {
        state: "off",
        message: "Chat is turned off.",
        action: { kind: "settings", section: "ai", setting: "ai.chat.enabled" },
      };
    }
    const model = chatModel().trim();
    if (!model) {
      return {
        state: "no_model",
        message: "No model is set.",
        action: { kind: "settings", section: "ai", setting: "ai.model" },
      };
    }
    const probe = aiConnectionStore.status();
    if (probe?.kind === "consent_required") {
      return {
        state: "unconsented",
        message: `${probe.detail} has not been allowed yet.`,
        action: { kind: "settings", section: "ai", setting: "ai.provider" },
      };
    }
    if (probe?.kind === "key_required") {
      return {
        state: "no_key",
        message: "Add an API key to use this connection.",
        action: { kind: "settings", section: "ai", setting: "ai.api_key" },
      };
    }
    if (probe?.kind === "refused" && aiConnectionStore.isLocal()) {
      return {
        state: "offline_local",
        message: `Nothing is answering at ${probe.detail}.`,
        action: { kind: "check" },
      };
    }
    const catalog = aiConnectionStore.catalog();
    if (catalog?.source === "live" && !catalog.models.includes(model)) {
      return {
        state: "model_unavailable",
        message: `${model} is not available on ${catalog.provider}.`,
        action: { kind: "settings", section: "ai", setting: "ai.model" },
      };
    }
    return { state: "ready" };
  }

  /** Copies one code block's source, which is what the model wrote rather
   * than the markup it was rendered into. Answers whether it landed, so the
   * button can say so. */
  async function copyCode(text: string): Promise<boolean> {
    try {
      await writeClipboardText(text);
      return true;
    } catch {
      showToast("Could not copy the code.", "error");
      return false;
    }
  }

  async function refreshList() {
    try {
      setConversations(await chatList());
    } catch {
      setConversations([]);
    }
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

  /** Loads the list and shows the conversation last written to.
   *
   * The file is the conversation (ADR-040 decision 1), so opening the pane
   * reads one rather than starting one: a chat closed yesterday is on screen
   * where it was left. */
  async function openPane() {
    aiConnectionStore.watch();
    watchNotes();
    await refreshList();
    if (current() || isBusy()) return;
    const recent = conversations()[0];
    if (recent) await open(recent.id);
  }

  /** Puts a conversation on screen. Nothing else moves: a reply arriving into
   * the conversation being left keeps arriving, into its own entry. */
  async function open(id: string) {
    let conversation: ChatConversation;
    try {
      conversation = await chatOpen(id);
    } catch (error) {
      failWith(readableError(error));
      return;
    }
    clearView();
    // The notes belong to the chat they were attached in (R5 design item 7).
    setAttachments([]);
    ensureEntry(id);
    setCurrent(conversation);
    await renderStoredReplies(conversation);
  }

  /** Puts the pane on a conversation that does not exist yet. The next send
   * writes the file, so a chat nobody used leaves nothing behind. */
  function newChat() {
    clearView();
    // A new chat starts with the note in front and nothing the last chat
    // carried, so it counts as an opening.
    setAttachments([]);
    setAttachGeneration((count) => count + 1);
  }

  async function rename(id: string, title: string) {
    try {
      const renamed = await chatRename(id, title);
      if (current()?.id === id) setCurrent((held) => (held ? { ...held, ...renamed } : renamed));
      await refreshList();
    } catch (error) {
      failWith(readableError(error));
    }
  }

  /** Deletes a conversation, stopping its reply first: the file is about to
   * go, and a task still writing to it could only warn. */
  async function remove(id: string) {
    const entry = entryOf(id);
    if (entry && isLive(id)) await chatStop(id, entry.requestId).catch(() => undefined);
    try {
      await chatDelete(id);
    } catch (error) {
      failWith(readableError(error));
      return;
    }
    dropEntry(id);
    if (current()?.id === id) {
      clearView();
      setAttachments([]);
    }
    await refreshList();
  }

  /** Sends the draft. The caller has already cleared the blockers, so this
   * only names the conversation and hands the message over.
   *
   * `resolved` is the list the send dialog counted. It is passed in rather
   * than read again, so the bytes a person agreed to are the bytes that leave
   * the machine (R5 M5). */
  async function send(resolved?: Attachment[]) {
    const text = draft().trim();
    // The status says nothing until the notes have been read off disk, so the
    // latch is what a second Send before that meets.
    if (!text || isBusy() || sending) return;
    sending = true;
    try {
      await sendTyped(text, resolved);
    } finally {
      sending = false;
    }
  }

  async function sendTyped(text: string, resolved?: Attachment[]) {
    let conversation = current();
    if (!conversation) {
      try {
        conversation = await chatNew();
      } catch (error) {
        failWith(readableError(error));
        return;
      }
      const started = conversation;
      clearView();
      ensureEntry(started.id);
      setCurrent(started);
      void refreshList();
    }

    // The paths come from the list the dialog counted, so one note is one path
    // in the request however each writer of the list spelled it.
    const notes = resolved ?? (await attachedOnDisk());
    const paths = notes.map((note) => note.path);
    const truncateTo = editing();
    const kept = truncateTo === null ? conversation.turns : conversation.turns.slice(0, truncateTo);
    if (truncateTo !== null) setCurrent({ ...conversation, turns: kept });
    const id = conversation.id;
    const requestId = beginExchange(id, kept.length, text, paths);

    try {
      await chatSend(id, text, paths, truncateTo ?? undefined, requestId);
    } catch (error) {
      // A send that was never accepted leaves nothing on screen and the words
      // back in the composer, to send again or edit. The turns an edit would
      // have replaced are still in the file, so they are still shown and the
      // next send still replaces them rather than being added after them.
      // Only the exchange this call opened is cleared: a newer one on the same
      // conversation is still streaming.
      if (entryOf(id)?.requestId !== requestId) return;
      failSend(id, readableError(error));
      if (current()?.id === id) {
        setCurrent(conversation);
        setEditing(truncateTo);
        setDraft(text);
      }
    }
  }

  /** Sends the last message again, in place of the turn that failed. */
  async function retry() {
    const again = retryable();
    const conversation = current();
    // Retry takes the same latch Send holds, so the two cannot both open an
    // exchange on one conversation.
    if (!again || !conversation || isBusy() || sending) return;
    sending = true;
    const typed = draft();
    const id = conversation.id;
    try {
      setCurrent({ ...conversation, turns: conversation.turns.slice(0, again.turn) });
      const requestId = beginExchange(id, again.turn, again.text, again.paths);
      try {
        await chatSend(id, again.text, again.paths, again.turn, requestId);
      } catch (error) {
        // The retry was never accepted, so the turn it would have replaced is
        // still in the file and stays on screen, and anything half-typed is
        // still in the composer.
        if (entryOf(id)?.requestId !== requestId) return;
        failSend(id, readableError(error));
        if (current()?.id === id) {
          setCurrent(conversation);
          setDraft(typed);
        }
      }
    } finally {
      sending = false;
    }
  }

  function failSend(id: string, message: string) {
    patchEntry(id, {
      requestId: "",
      user: null,
      reply: null,
      status: "error",
      errorMessage: message,
      errorKind: "",
      errorIdentity: null,
    });
  }

  /** Opens one exchange on a conversation and answers the id it was minted
   * with, which is what every frame of it must carry. */
  function beginExchange(id: string, userTurn: number, text: string, paths: string[]): string {
    const entry = ensureEntry(id);
    // A resend reuses the index the failed reply held, so its fragment goes
    // with it rather than sitting under the new one.
    const html = { ...entry.htmlByTurn };
    delete html[userTurn + 1];
    renderAsked.delete(`${id}:${userTurn + 1}`);
    const requestId = newRequestId();
    patchEntry(id, {
      requestId,
      htmlByTurn: html,
      user: {
        turn: userTurn,
        role: "user",
        content: text,
        html: "",
        attachments: paths.map((path) => ({ path, bytes: 0, hash: "" })),
        proposals: [],
        dropped: [],
        truncated: false,
        identity: null,
      },
      reply: {
        turn: userTurn + 1,
        role: "assistant",
        content: "",
        html: "",
        attachments: [],
        proposals: [],
        dropped: [],
        truncated: false,
        identity: null,
      },
      lastSend: { turn: userTurn, text, paths },
      status: "thinking",
      errorMessage: "",
      errorKind: "",
      errorIdentity: null,
    });
    setPaneError("");
    setDraft("");
    setEditing(null);
    return requestId;
  }

  /** Puts a sent turn back in the composer. Sending it again replaces it and
   * everything after it, which is what the file then holds. */
  async function beginEdit(turn: number) {
    const stored = current()?.turns[turn];
    if (!stored || stored.role !== "user" || isBusy()) return;
    setEditing(turn);
    setDraft(stored.content);
    setAttachments([]);
    await attachAll(
      stored.attachments.map((note) => ({
        path: note.path,
        name: noteName(note.path),
        bytes: note.bytes,
      })),
    );
  }

  function cancelEdit() {
    setEditing(null);
    setDraft("");
  }

  /** Stops a live reply, by the request id that is streaming rather than by
   * whatever the pane happens to show. The text already on screen stays, and
   * the reply it came from produced no proposal, so there is nothing to
   * apply. */
  function stop(id: string | undefined = current()?.id) {
    if (!id) return;
    const entry = entryOf(id);
    if (!entry || !isLive(id)) return;
    void chatStop(id, entry.requestId).catch(() => undefined);
  }

  /** Routes one frame to the exchange it belongs to.
   *
   * The pair (conversation id, request id) is the whole of the routing: a
   * frame for a conversation nobody is waiting on, or for a send that has
   * been superseded, is a leftover and changes nothing. */
  function handleStreamEvent(payload: ChatPayload) {
    if (!payload) return;
    const id = payload.conversation_id;
    const entry = entryOf(id);
    if (!entry || !entry.requestId || entry.requestId !== payload.request_id) return;
    if (payload.kind === "chunk") {
      if (entry.status !== "thinking" && entry.status !== "streaming") return;
      appendToReply(id, payload.text ?? "");
      patchEntry(id, { status: "streaming" });
    } else if (payload.kind === "done") {
      if (entry.reply) {
        patchEntry(id, {
          reply: {
            ...entry.reply,
            proposals: payload.proposals ?? [],
            dropped: payload.dropped ?? [],
            truncated: payload.truncated ?? false,
            identity: payload.identity ?? null,
          },
        });
      }
      settle(id, "done");
    } else if (payload.kind === "stopped") {
      settle(id, "stopped");
    } else if (payload.kind === "error") {
      const typed = payload.error;
      patchEntry(id, {
        errorMessage: typed?.message ?? payload.text ?? "The reply did not arrive.",
        errorKind: typed?.kind ?? "",
        errorIdentity: typed ? { provider: typed.provider, model: typed.model, host: "" } : null,
      });
      settle(id, "error");
    }
  }

  function appendToReply(id: string, text: string) {
    const entry = entryOf(id);
    if (!entry?.reply) return;
    patchEntry(id, { reply: { ...entry.reply, content: entry.reply.content + text } });
    scheduleRender(id);
  }

  /** The stream is over, so the turns are in the file. The local fold keeps
   * the reply on screen at the indices Rust used; the reload that follows
   * replaces it with what the file actually holds.
   *
   * A conversation the pane is not showing has nothing to fold into: its file
   * already holds the reply, so the list is read again and its row says so. */
  function settle(id: string, ending: ChatStatus) {
    const entry = entryOf(id);
    if (!entry) return;
    cancelScheduledRender(entry);
    const { user, reply } = entry;
    const wrote = reply !== null && reply.content.length > 0;
    const onScreen = current()?.id === id;
    const conversation = current();
    if (onScreen && user && conversation) {
      // Rust appends a reply only when it holds text, so an ending with
      // nothing shown leaves the user turn last in the file. A turn the file
      // already holds is not folded in a second time.
      const turns = [...conversation.turns];
      if (turns.length <= user.turn) turns.push(storedTurn(user));
      if (wrote && reply && turns.length <= reply.turn) turns.push(storedTurn(reply));
      setCurrent({ ...conversation, turns });
    }
    patchEntry(id, { status: ending, requestId: "", user: null, reply: null });
    if (wrote && reply) void renderTurn(id, reply.turn, reply.content);
    if (onScreen) void reload(id);
    else void refreshList();
  }

  function storedTurn(message: Message): ChatStoredTurn {
    return {
      role: message.role,
      content: message.content,
      attachments: message.attachments,
      proposals: message.proposals,
      dropped: message.dropped,
      truncated: message.truncated,
    };
  }

  /** Reads the conversation back from the file it was written to. */
  async function reload(id: string) {
    let conversation: ChatConversation;
    try {
      conversation = await chatOpen(id);
    } catch {
      return;
    }
    if (current()?.id !== id) return;
    setCurrent(conversation);
    await renderStoredReplies(conversation);
    await refreshList();
  }

  async function renderStoredReplies(conversation: ChatConversation) {
    for (const [index, turn] of conversation.turns.entries()) {
      if (turn.role !== "assistant" || turn.content.length === 0) continue;
      await renderTurn(conversation.id, index, turn.content);
    }
  }

  function scheduleRender(id: string) {
    const entry = entryOf(id);
    if (!entry || entry.renderTimer.handle !== null) return;
    entry.renderTimer.handle = setTimeout(() => {
      entry.renderTimer.handle = null;
      const reply = entryOf(id)?.reply;
      if (!reply) return;
      void renderTurn(id, reply.turn, reply.content);
    }, RENDER_THROTTLE_MS);
  }

  function cancelScheduledRender(entry: Exchange) {
    if (entry.renderTimer.handle === null) return;
    clearTimeout(entry.renderTimer.handle);
    entry.renderTimer.handle = null;
  }

  /** Drops a record of a render that painted nothing, and only that record: a
   * newer render for the same turn keeps what it wrote. */
  function forgetRender(asked: string, markdown: string) {
    const held = renderAsked.get(asked);
    if (held?.markdown === markdown && held.html === null) renderAsked.delete(asked);
  }

  /** Renders one reply to a fragment, into the conversation that asked for
   * it. A result a newer render has already overtaken is dropped rather than
   * painted, and a conversation the pane has left still gets its fragment. */
  async function renderTurn(id: string, turn: number, markdown: string) {
    const entry = entryOf(id);
    if (!entry) return;
    const asked = `${id}:${turn}`;
    const held = renderAsked.get(asked);
    if (
      held?.markdown === markdown &&
      (held.html === null || entry.htmlByTurn[turn] === held.html)
    ) {
      return;
    }
    renderAsked.set(asked, { markdown, html: null });
    const generation = (entry.renderGeneration.get(turn) ?? 0) + 1;
    entry.renderGeneration.set(turn, generation);
    let html: string;
    try {
      html = await chatRenderReply(markdown);
    } catch {
      // Nothing was painted, so the next ask for this text is a fresh one.
      forgetRender(asked, markdown);
      return;
    }
    const still = entryOf(id);
    if (!still || still.renderGeneration.get(turn) !== generation) {
      forgetRender(asked, markdown);
      return;
    }
    renderAsked.set(asked, { markdown, html });
    patchEntry(id, { htmlByTurn: { ...still.htmlByTurn, [turn]: html } });
  }

  /** Why applying was refused, for the proposal it was refused for. */
  function refusalFor(turn: number, path: string): string | undefined {
    return refusals()[`${turn}:${path}`];
  }

  /** Whether that proposal's write is in flight. */
  function isApplying(turn: number, path: string): boolean {
    return applying.has(`${turn}:${path}`);
  }

  function setProposalStatus(turn: number, path: string, next: ChatProposal["status"]) {
    setCurrent((held) => {
      if (!held) return held;
      return {
        ...held,
        turns: held.turns.map((stored, index) =>
          index === turn
            ? {
                ...stored,
                proposals: stored.proposals.map((proposal) =>
                  proposal.path === path ? { ...proposal, status: next } : proposal,
                ),
              }
            : stored,
        ),
      };
    });
  }

  function refuse(turn: number, path: string, why: string) {
    setRefusals((held) => ({ ...held, [`${turn}:${path}`]: why }));
  }

  /** Writes one proposal, through the guarded facade and nothing else.
   *
   * A note that changed since the proposal was made is refused there, and the
   * refusal is what the card then shows: the note is as its last writer left
   * it, and the proposed text is still in the conversation. */
  async function apply(turn: number, proposal: ChatProposal): Promise<ChatProposalOutcome | null> {
    const id = current()?.id;
    const key = `${turn}:${proposal.path}`;
    if (!id) {
      refuse(turn, proposal.path, "This chat is no longer open.");
      return null;
    }
    // Applying is one-shot: a second press while the write runs does nothing.
    if (applying.has(key)) return null;
    applying.add(key);
    try {
      const outcome = await chatApplyProposal(
        id,
        turn,
        proposal.path,
        proposal.new_content,
        proposal.before_hash,
      );
      setProposalStatus(turn, proposal.path, "applied");
      // The note is a different size now, and a chip for it must state the
      // size the next message will carry.
      if (outcome.changed) void refreshAttachedSizes();
      return outcome;
    } catch (error) {
      refuse(turn, proposal.path, readableError(error));
      setProposalStatus(turn, proposal.path, "refused");
      return null;
    } finally {
      applying.delete(key);
    }
  }

  /** Puts a proposal aside. The file records the verdict, so the pane says so
   * once the record is written and not before. */
  async function discard(turn: number, proposal: ChatProposal) {
    const id = current()?.id;
    if (!id) {
      refuse(turn, proposal.path, "This chat is no longer open.");
      return;
    }
    try {
      await chatDiscardProposal(id, turn, proposal.path);
    } catch (error) {
      refuse(turn, proposal.path, readableError(error));
      return;
    }
    setProposalStatus(turn, proposal.path, "discarded");
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
   * nothing (R5 M3). */
  async function attachedOnDisk(): Promise<Attachment[]> {
    const held = attachments();
    if (held.length === 0) return [];
    const sizes = await chatAttachedSizes(held.map((note) => note.path)).catch(() => null);
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

  return {
    conversations,
    current,
    messages,
    liveModels,
    readiness,
    copyCode,
    attachments,
    attachGeneration,
    status,
    statusOf,
    isLive,
    liveConversations,
    errorMessage,
    errorKind,
    errorIdentity,
    canRetry,
    draft,
    setDraft,
    editing,
    refusalFor,
    isApplying,
    attach,
    attachAll,
    attachByPath,
    attachAuto,
    addOpenNote,
    detach,
    isAttached,
    openPane,
    refreshList,
    open,
    newChat,
    rename,
    remove,
    send,
    retry,
    stop,
    beginEdit,
    cancelEdit,
    apply,
    discard,
    reset,
    handleStreamEvent,
    /** Where the chat endpoint points and what it still needs. */
    endpointState: (): Promise<ChatEndpointState> => chatState(),
    attachedOnDisk,
    refreshAttachedSizes,
    setAttachedList,
  };
}

function readableError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "The reply did not arrive.";
}

export type ChatStore = ReturnType<typeof createChatStore>;
export const chatStore = createRoot(createChatStore);
