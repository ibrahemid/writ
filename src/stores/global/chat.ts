import { createSignal, createRoot } from "solid-js";
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
  chatCancel,
  chatApplyProposal,
  chatDiscardProposal,
  type ChatEndpointState,
  type ChatAttachmentRef,
  type ChatConversation,
  type ChatConversationSummary,
  type ChatProposal,
  type ChatProposalOutcome,
  type ChatStoredTurn,
  type DiffHunk,
  type DiffLine,
} from "../../services/tauri";
import { aiConnectionStore } from "./ai-connection";
import { showToast } from "../../components/Notifications/Toast";
import { writeClipboardText } from "../../services/clipboard";
import type { WritEvent } from "../../types/events";

export type {
  ChatAttachmentRef,
  ChatConversation,
  ChatConversationSummary,
  ChatProposal,
  DiffHunk,
  DiffLine,
};

type ChatPayload = Extract<WritEvent, { kind: "ai:chat" }>["payload"];

export type ChatStatus = "idle" | "thinking" | "streaming" | "done" | "stopped" | "error";

/** How often a live reply is re-rendered while it streams (ADR-040 section 9). */
export const RENDER_THROTTLE_MS = 120;

/** A note the conversation carries, as the pane lists it. */
export interface Attachment {
  /** Absolute path, which is what the send names. */
  path: string;
  /** The file's name, which is what the row shows. */
  name: string;
  /** The note's size on disk. */
  bytes: number;
}

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

// Singleton state — Writ is single-window. One conversation is open at a time,
// and the pane is the only thing that shows it.
function createChatStore() {
  const [conversations, setConversations] = createSignal<ChatConversationSummary[]>([]);
  const [current, setCurrent] = createSignal<ChatConversation | null>(null);
  // The turns of a send Rust has not written yet. They render after the stored
  // ones, at the indices Rust will give them.
  const [pendingUser, setPendingUser] = createSignal<Message | null>(null);
  const [pendingReply, setPendingReply] = createSignal<Message | null>(null);
  const [htmlByTurn, setHtmlByTurn] = createSignal<Record<number, string>>({});
  const [attachments, setAttachments] = createSignal<Attachment[]>([]);
  const [status, setStatus] = createSignal<ChatStatus>("idle");
  const [errorMessage, setErrorMessage] = createSignal("");
  const [draft, setDraft] = createSignal("");
  const [editing, setEditing] = createSignal<number | null>(null);
  const [refusals, setRefusals] = createSignal<Record<string, string>>({});
  const [liveModels, setLiveModels] = createSignal<string[]>([]);
  const [lastSend, setLastSend] = createSignal<{
    turn: number;
    text: string;
    paths: string[];
  } | null>(null);

  // A render is one round trip and a stream asks for many, so only the newest
  // result for a turn is painted: a slow one cannot overwrite a newer one.
  const renderGeneration = new Map<number, number>();
  let renderTimer: ReturnType<typeof setTimeout> | null = null;

  function toMessage(turn: ChatStoredTurn, index: number): Message {
    return {
      turn: index,
      role: turn.role,
      content: turn.content,
      html: htmlByTurn()[index] ?? "",
      attachments: turn.attachments,
      proposals: turn.proposals,
    };
  }

  function messages(): Message[] {
    const conversation = current();
    const shown = conversation ? conversation.turns.map(toMessage) : [];
    const user = pendingUser();
    const reply = pendingReply();
    if (user) shown.push(user);
    if (reply) shown.push({ ...reply, html: htmlByTurn()[reply.turn] ?? "" });
    return shown;
  }

  function isBusy(): boolean {
    return status() === "thinking" || status() === "streaming";
  }

  function reset() {
    cancelScheduledRender();
    setCurrent(null);
    setPendingUser(null);
    setPendingReply(null);
    setHtmlByTurn({});
    setRefusals({});
    setLastSend(null);
    setEditing(null);
    setStatus("idle");
    setErrorMessage("");
    setDraft("");
    renderGeneration.clear();
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

  /** Attaches a note the pane knows only a path for, reading its size from
   * disk so the chip and the send dialog state the same number. */
  async function attachByPath(path: string) {
    if (isAttached(path)) return;
    let bytes = 0;
    try {
      bytes = (await chatAttachedSizes([path]))[0]?.bytes ?? 0;
    } catch {
      bytes = 0;
    }
    attach({ path, name: noteName(path), bytes });
  }

  /** The models the endpoint itself lists, for the picker.
   *
   * A hosted endpoint is not asked before its host has been consented to: the
   * list is a request to that host, so the picker falls back to the curated
   * ids rather than reaching one nobody has agreed to (ADR-040 section 10). */
  async function loadModels() {
    try {
      const endpoint = await chatState();
      if (endpoint.is_hosted && !endpoint.is_consented) {
        setLiveModels([]);
        return;
      }
    } catch {
      setLiveModels([]);
      return;
    }
    const listed = await aiConnectionStore.listModels();
    setLiveModels("models" in listed ? listed.models : []);
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

  /** Loads the list and shows the conversation last written to.
   *
   * The file is the conversation (ADR-040 decision 1), so opening the pane
   * reads one rather than starting one: a chat closed yesterday is on screen
   * where it was left. */
  async function openPane() {
    void loadModels();
    await refreshList();
    if (current() || isBusy()) return;
    const recent = conversations()[0];
    if (recent) await open(recent.id);
  }

  async function open(id: string) {
    let conversation: ChatConversation;
    try {
      conversation = await chatOpen(id);
    } catch (error) {
      setStatus("error");
      setErrorMessage(readableError(error));
      return;
    }
    reset();
    setCurrent(conversation);
    await renderStoredReplies(conversation);
  }

  /** Puts the pane on a conversation that does not exist yet. The next send
   * writes the file, so a chat nobody used leaves nothing behind. */
  function newChat() {
    if (isBusy()) return;
    reset();
  }

  async function rename(id: string, title: string) {
    try {
      const renamed = await chatRename(id, title);
      if (current()?.id === id) setCurrent((held) => (held ? { ...held, ...renamed } : renamed));
      await refreshList();
    } catch (error) {
      setErrorMessage(readableError(error));
    }
  }

  async function remove(id: string) {
    try {
      await chatDelete(id);
    } catch (error) {
      setErrorMessage(readableError(error));
      return;
    }
    if (current()?.id === id) reset();
    await refreshList();
  }

  /** Sends the draft. The caller has already cleared the blockers, so this
   * only names the conversation and hands the message over.
   *
   * The turns live in a file Rust owns, so the send names the conversation
   * rather than replaying it: a conversation that does not exist yet is
   * created here, on the first send. */
  async function send() {
    const text = draft().trim();
    if (!text || isBusy()) return;

    let conversation = current();
    if (!conversation) {
      try {
        conversation = await chatNew();
      } catch (error) {
        setStatus("error");
        setErrorMessage(readableError(error));
        return;
      }
      const started = conversation;
      reset();
      setCurrent(started);
      void refreshList();
    }

    const truncateTo = editing();
    const kept = truncateTo === null ? conversation.turns : conversation.turns.slice(0, truncateTo);
    if (truncateTo !== null) setCurrent({ ...conversation, turns: kept });
    const paths = attachments().map((note) => note.path);
    beginExchange(kept.length, text, paths);

    try {
      await chatSend(conversation.id, text, paths, truncateTo ?? undefined);
    } catch (error) {
      // A send that was never accepted leaves nothing on screen and the words
      // back in the composer, to send again or edit.
      setPendingUser(null);
      setPendingReply(null);
      setDraft(text);
      setStatus("error");
      setErrorMessage(readableError(error));
    }
  }

  /** Sends the last message again, in place of the turn that failed. */
  async function retry() {
    const again = lastSend();
    const conversation = current();
    if (!again || !conversation || isBusy()) return;
    setCurrent({ ...conversation, turns: conversation.turns.slice(0, again.turn) });
    beginExchange(again.turn, again.text, again.paths);
    try {
      await chatSend(conversation.id, again.text, again.paths, again.turn);
    } catch (error) {
      setPendingUser(null);
      setPendingReply(null);
      setStatus("error");
      setErrorMessage(readableError(error));
    }
  }

  function beginExchange(userTurn: number, text: string, paths: string[]) {
    // A resend reuses the index the failed reply held, so its fragment goes
    // with it rather than sitting under the new one.
    setHtmlByTurn((held) => {
      const next = { ...held };
      delete next[userTurn + 1];
      return next;
    });
    setPendingUser({
      turn: userTurn,
      role: "user",
      content: text,
      html: "",
      attachments: paths.map((path) => ({ path, bytes: 0, hash: "" })),
      proposals: [],
    });
    setPendingReply({
      turn: userTurn + 1,
      role: "assistant",
      content: "",
      html: "",
      attachments: [],
      proposals: [],
    });
    setLastSend({ turn: userTurn, text, paths });
    setDraft("");
    setEditing(null);
    setErrorMessage("");
    setStatus("thinking");
  }

  /** Puts a sent turn back in the composer. Sending it again replaces it and
   * everything after it, which is what the file then holds. */
  function beginEdit(turn: number) {
    const stored = current()?.turns[turn];
    if (!stored || stored.role !== "user" || isBusy()) return;
    setEditing(turn);
    setDraft(stored.content);
    setAttachments(
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

  /** Stops a live reply. The text already on screen stays, and the reply it
   * came from produced no proposal, so there is nothing to apply. */
  function stop() {
    const id = current()?.id;
    if (!id || !isBusy()) return;
    void chatCancel(id);
  }

  function handleStreamEvent(payload: ChatPayload) {
    if (!payload || payload.conversation_id !== current()?.id) return;
    if (payload.kind === "chunk") {
      if (!isBusy()) return;
      setStatus("streaming");
      appendToReply(payload.text ?? "");
    } else if (payload.kind === "done") {
      if (!isBusy()) return;
      setPendingReply((held) => (held ? { ...held, proposals: payload.proposals ?? [] } : held));
      settle("done");
    } else if (payload.kind === "stopped") {
      settle("stopped");
    } else if (payload.kind === "error") {
      setErrorMessage(payload.text ?? "The reply did not arrive.");
      settle("error");
    }
  }

  function appendToReply(text: string) {
    const reply = pendingReply();
    if (!reply) return;
    setPendingReply({ ...reply, content: reply.content + text });
    scheduleRender();
  }

  /** The stream is over, so the turns are in the file. The local fold keeps
   * the reply on screen at the indices Rust used; the reload that follows
   * replaces it with what the file actually holds. */
  function settle(ending: ChatStatus) {
    cancelScheduledRender();
    setStatus(ending);
    const conversation = current();
    const user = pendingUser();
    const reply = pendingReply();
    if (!conversation || !user) return;
    // Rust appends a reply only when it holds text, so an ending with nothing
    // shown leaves the user turn last in the file.
    const turns = [...conversation.turns, storedTurn(user)];
    if (reply && reply.content.length > 0) turns.push(storedTurn(reply));
    setCurrent({ ...conversation, turns });
    setPendingUser(null);
    setPendingReply(null);
    if (reply && reply.content.length > 0) {
      void renderTurn(conversation.id, reply.turn, reply.content);
    }
    void reload(conversation.id);
  }

  function storedTurn(message: Message): ChatStoredTurn {
    return {
      role: message.role,
      content: message.content,
      attachments: message.attachments,
      proposals: message.proposals,
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

  function scheduleRender() {
    if (renderTimer !== null) return;
    renderTimer = setTimeout(() => {
      renderTimer = null;
      const reply = pendingReply();
      const id = current()?.id;
      if (!reply || !id) return;
      void renderTurn(id, reply.turn, reply.content);
    }, RENDER_THROTTLE_MS);
  }

  function cancelScheduledRender() {
    if (renderTimer === null) return;
    clearTimeout(renderTimer);
    renderTimer = null;
  }

  /** Renders one reply to a fragment. A result a newer render has already
   * overtaken is dropped rather than painted. */
  async function renderTurn(id: string, turn: number, markdown: string) {
    const generation = (renderGeneration.get(turn) ?? 0) + 1;
    renderGeneration.set(turn, generation);
    let html: string;
    try {
      html = await chatRenderReply(markdown);
    } catch {
      return;
    }
    if (renderGeneration.get(turn) !== generation || current()?.id !== id) return;
    setHtmlByTurn((held) => ({ ...held, [turn]: html }));
  }

  /** Why applying was refused, for the proposal it was refused for. */
  function refusalFor(turn: number, path: string): string | undefined {
    return refusals()[`${turn}:${path}`];
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

  /** Writes one proposal, through the guarded facade and nothing else.
   *
   * A note that changed since the proposal was made is refused there, and the
   * refusal is what the card then shows: the note is as its last writer left
   * it, and the proposed text is still in the conversation. */
  async function apply(turn: number, proposal: ChatProposal): Promise<ChatProposalOutcome | null> {
    const id = current()?.id;
    if (!id) return null;
    try {
      const outcome = await chatApplyProposal(
        id,
        turn,
        proposal.path,
        proposal.new_content,
        proposal.before_hash,
      );
      setProposalStatus(turn, proposal.path, "applied");
      return outcome;
    } catch (error) {
      setRefusals((held) => ({ ...held, [`${turn}:${proposal.path}`]: readableError(error) }));
      setProposalStatus(turn, proposal.path, "refused");
      return null;
    }
  }

  async function discard(turn: number, proposal: ChatProposal) {
    const id = current()?.id;
    if (!id) return;
    setProposalStatus(turn, proposal.path, "discarded");
    await chatDiscardProposal(id, turn, proposal.path).catch(() => undefined);
  }

  return {
    conversations,
    current,
    messages,
    liveModels,
    loadModels,
    copyCode,
    attachments,
    status,
    errorMessage,
    draft,
    setDraft,
    editing,
    refusalFor,
    attach,
    attachByPath,
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
    /** The attached notes with the sizes the files hold now.
     *
     * A tab records a note's size when it reads it, and another program can
     * rewrite the file after that. The dialog asking to send it must state the
     * bytes the send will carry, so it asks disk rather than the tab. */
    async attachedOnDisk(): Promise<Attachment[]> {
      const held = attachments();
      const sizes = await chatAttachedSizes(held.map((note) => note.path));
      // Keyed by the path that was asked about, which is the absolute one
      // these attachments hold. The command's own folder-relative key names
      // the same note in a different shape and would miss every row.
      const byPath = new Map(sizes.map((note) => [note.path, note.bytes]));
      return held.map((note) => ({ ...note, bytes: byPath.get(note.path) ?? note.bytes }));
    },
  };
}

function readableError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "The reply did not arrive.";
}

export type ChatStore = ReturnType<typeof createChatStore>;
export const chatStore = createRoot(createChatStore);
