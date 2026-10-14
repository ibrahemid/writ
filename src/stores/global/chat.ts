import { createSignal, createMemo, createRoot } from "solid-js";
import {
  chatState,
  chatList,
  chatOpen,
  chatRename,
  chatDelete,
  chatStop,
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
import { aiConnectionStore } from "./ai-connection";
import { aiProvidersStore, ON_DEVICE_PROVIDER, REWRITE_ONLY_MESSAGE } from "./ai-providers";
import { configStore } from "./config";
import { showToast } from "./notifications";
import { writeClipboardText } from "../../services/tauri";
import { createAttachments, noteName } from "./chat/attachments";
import { createEntries, createExchange, type ChatView, type Message } from "./chat/exchange";
import { createProposals } from "./chat/proposals";
import { createRender } from "./chat/render";
import { readableError } from "./chat/errors";

export type {
  ChatAttachmentRef,
  ChatConversation,
  ChatConversationSummary,
  ChatDroppedProposal,
  ChatProposal,
  ChatProposalOutcome,
  DiffHunk,
  DiffLine,
  RequestIdentity,
};

export {
  chipLabel,
  chipRows,
  folderChipLabel,
  isAbsolutePath,
  noteKeyLabel,
  noteName,
  proposalLabel,
  totalBytes,
} from "./chat/attachments";
export type {
  AddOpenNoteRefusal,
  AddOpenNoteResult,
  AttachFolderResult,
  Attachment,
  AttachmentState,
  ChipRow,
  FrontTab,
} from "./chat/attachments";
export type { ChatStatus, Message } from "./chat/exchange";
export type { ProposalVerdict } from "./chat/proposals";
export { RENDER_THROTTLE_MS } from "./chat/render";

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
    held.writing === next.writing &&
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
  | "unconsented"
  | "rewrite_only";

/** What pressing the readiness line does. */
export type ReadinessAction =
  | { kind: "settings"; section: "apps"; setting: string }
  | { kind: "check" };

export type Readiness =
  | { state: "ready" }
  | { state: Exclude<ReadinessState, "ready">; message: string; action: ReadinessAction };

// Singleton state — Writ is single-window. One conversation is on screen at a
// time; the entries behind it are per conversation, because a reply keeps
// arriving into the conversation it was sent from.
function createChatStore() {
  const [conversations, setConversations] = createSignal<ChatConversationSummary[]>([]);
  // Whether the last read of the list failed, so the pane can say so rather
  // than show an empty list as if there were no chats.
  const [listFailed, setListFailed] = createSignal(false);
  const [current, setCurrent] = createSignal<ChatConversation | null>(null);
  // A failure that belongs to the pane rather than to any one conversation:
  // opening, renaming and deleting all end here, and none of them may write
  // over the status of an exchange that is still streaming.
  const [paneError, setPaneError] = createSignal("");
  const [draft, setDraft] = createSignal("");
  const [editing, setEditing] = createSignal<number | null>(null);
  // True while a stored conversation is being read and rendered, so the pane
  // shows nothing rather than the copy that belongs to a chat with no turns.
  const [loading, setLoading] = createSignal(false);

  const view: ChatView = {
    current,
    setCurrent,
    draft,
    setDraft: (text) => setDraft(text),
    editing,
    setEditing: (turn) => setEditing(turn),
    setPaneError: (message) => setPaneError(message),
  };
  const entries = createEntries();
  const render = createRender(entries);
  const chips = createAttachments();
  const proposals = createProposals(view, {
    refreshAttachedSizes: () => chips.refreshAttachedSizes(),
  });
  const exchange = createExchange(view, entries, render, {
    attachedOnDisk: () => chips.attachedOnDisk(),
    refreshList: () => refreshList(),
    clearView: () => clearView(),
    failWith: (message) => failWith(message),
  });

  const currentEntry = exchange.currentEntry;

  function dropEntry(id: string): void {
    const held = entries.entryOf(id);
    if (held) render.cancelScheduledRender(held);
    entries.removeEntry(id);
    render.forgetConversation(id);
  }

  function toMessage(
    turn: ChatStoredTurn,
    index: number,
    html: string,
    identity: RequestIdentity | null,
  ): Message {
    return {
      turn: index,
      role: turn.role,
      content: turn.content,
      html,
      attachments: turn.attachments,
      proposals: turn.proposals,
      dropped: turn.dropped ?? [],
      truncated: turn.truncated ?? false,
      identity,
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
    const known = currentEntry()?.identityByTurn ?? {};
    const shown = turns.map((turn, index) =>
      toMessage(turn, index, html[index], known[index] ?? null),
    );
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

  function status() {
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

  /** Whether the pane holds a message it could send again. */
  function canRetry(): boolean {
    return exchange.retryable() !== null;
  }

  /** Shows a failure that belongs to the pane and to no conversation. */
  function failWith(message: string) {
    setPaneError(message);
  }

  /** Clears the view without touching any conversation's entry or the chips.
   *
   * The draft stays: a question typed before New chat, or before opening
   * another chat, is still the question (CH-24). A turn being edited is the
   * exception, because its words belong to the conversation it came from. */
  function clearView() {
    setCurrent(null);
    proposals.clearRefusals();
    setPaneError("");
    if (editing() !== null) {
      setEditing(null);
      setDraft("");
    }
  }

  /** Puts the pane back to holding nothing: no conversation on screen and no
   * session state behind one. The chips are left alone, because they are
   * cleared where a conversation changes (R5 design item 7). */
  function reset() {
    for (const id of Object.keys(entries.exchanges())) dropEntry(id);
    clearView();
    setDraft("");
    proposals.reset();
    chips.forgetFollow();
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
        action: { kind: "settings", section: "apps", setting: "ai.chat.enabled" },
      };
    }
    if (ai.provider === ON_DEVICE_PROVIDER) {
      return {
        state: "rewrite_only",
        message: REWRITE_ONLY_MESSAGE,
        action: { kind: "settings", section: "apps", setting: "ai.provider" },
      };
    }
    const model = chatModel().trim();
    if (!model) {
      return {
        state: "no_model",
        message: "No model is set.",
        action: { kind: "settings", section: "apps", setting: "ai.model" },
      };
    }
    const probe = aiConnectionStore.status();
    if (probe?.kind === "consent_required") {
      return {
        state: "unconsented",
        message: `${probe.detail} has not been allowed yet.`,
        action: { kind: "settings", section: "apps", setting: "ai.provider" },
      };
    }
    if (probe?.kind === "key_required") {
      return {
        state: "no_key",
        message: "Add an API key to use this connection.",
        action: { kind: "settings", section: "apps", setting: "ai.api_key" },
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
        message: `${model} is not available on ${aiProvidersStore.byId(catalog.provider)?.label ?? catalog.provider}.`,
        action: { kind: "settings", section: "apps", setting: "ai.model" },
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
      setListFailed(false);
    } catch {
      setConversations([]);
      setListFailed(true);
    }
  }

  /** Loads the list and shows the conversation last written to.
   *
   * The file is the conversation (ADR-040 decision 1), so opening the pane
   * reads one rather than starting one: a chat closed yesterday is on screen
   * where it was left. The connection is checked as the pane opens, so the
   * readiness line states what a send would meet rather than waiting for one
   * to fail (CH-22). */
  async function openPane() {
    aiConnectionStore.watch();
    if (aiConnectionStore.status() === null && !aiConnectionStore.checking()) {
      void aiConnectionStore.check();
    }
    chips.watchNotes();
    proposals.watchOfferedNotes();
    await refreshList();
    if (current() || exchange.isBusy()) return;
    const recent = conversations()[0];
    if (recent) await open(recent.id);
  }

  /** Puts a conversation on screen. Nothing else moves: a reply arriving into
   * the conversation being left keeps arriving, into its own entry. */
  async function open(id: string) {
    let conversation: ChatConversation;
    setLoading(true);
    try {
      conversation = await chatOpen(id);
    } catch (error) {
      setLoading(false);
      failWith(readableError(error));
      return;
    }
    try {
      clearView();
      // The notes belong to the chat they were attached in (R5 design item 7).
      chips.clearChips();
      entries.ensureEntry(id);
      setCurrent(conversation);
      await render.renderStoredReplies(conversation);
    } finally {
      setLoading(false);
    }
  }

  /** Puts the pane on a conversation that does not exist yet. The next send
   * writes the file, so a chat nobody used leaves nothing behind. */
  function newChat() {
    clearView();
    // A new chat starts with the note in front and nothing the last chat
    // carried.
    chips.clearChips();
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
    const entry = entries.entryOf(id);
    if (entry && entries.isLive(id)) await chatStop(id, entry.requestId).catch(() => undefined);
    try {
      await chatDelete(id);
    } catch (error) {
      failWith(readableError(error));
      return;
    }
    dropEntry(id);
    if (current()?.id === id) {
      clearView();
      chips.clearChips();
    }
    await refreshList();
  }

  /** Puts a sent turn back in the composer. Sending it again replaces it and
   * everything after it, which is what the file then holds. */
  async function beginEdit(turn: number) {
    const stored = current()?.turns[turn];
    if (!stored || stored.role !== "user" || exchange.isBusy()) return;
    // The draft first: the composer moves its caret to the end of the field
    // when the turn is named, and the field has to hold the turn by then.
    setDraft(stored.content);
    setEditing(turn);
    chips.replaceWithNothing();
    await chips.attachAll(
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

  return {
    conversations,
    listFailed,
    current,
    messages,
    liveModels,
    readiness,
    loading,
    copyCode,
    attachments: chips.attachments,
    attachGeneration: chips.attachGeneration,
    /** Why the chip set as a whole cannot be sent, when no one note is why. */
    attachRefusal: chips.setRefusal,
    status,
    statusOf: entries.statusOf,
    isLive: entries.isLive,
    liveConversations: entries.liveConversations,
    errorMessage,
    errorKind,
    errorIdentity,
    canRetry,
    draft,
    setDraft,
    editing,
    refusalFor: proposals.refusalFor,
    isApplying: proposals.isApplying,
    verdictFor: proposals.verdictFor,
    attach: chips.attach,
    attachAll: chips.attachAll,
    attachByPath: chips.attachByPath,
    attachFolder: chips.attachFolder,
    detachFolder: chips.detachFolder,
    openTabCandidates: chips.openTabCandidates,
    followTab: chips.followTab,
    addOpenNote: chips.addOpenNote,
    detach: chips.detach,
    isAttached: chips.isAttached,
    openPane,
    refreshList,
    open,
    newChat,
    rename,
    remove,
    send: exchange.send,
    retry: exchange.retry,
    stop: exchange.stop,
    beginEdit,
    cancelEdit,
    apply: proposals.apply,
    undo: proposals.undo,
    openTarget: proposals.openTarget,
    discard: proposals.discard,
    reset,
    handleStreamEvent: exchange.handleStreamEvent,
    /** Where the chat endpoint points and what it still needs. */
    endpointState: (): Promise<ChatEndpointState> => chatState(),
    attachedOnDisk: chips.attachedOnDisk,
    refreshAttachedSizes: chips.refreshAttachedSizes,
    setAttachedList: chips.setAttachedList,
  };
}

export type ChatStore = ReturnType<typeof createChatStore>;
export const chatStore = createRoot(createChatStore);
