import { createSignal, type Accessor, type Setter } from "solid-js";
import {
  chatNew,
  chatOpen,
  chatSend,
  chatStop,
  type ChatAttachmentRef,
  type ChatConversation,
  type ChatDroppedProposal,
  type ChatProposal,
  type ChatStoredTurn,
  type RequestIdentity,
} from "../../../services/tauri";
import type { WritEvent } from "../../../types/events";
import type { Attachment } from "./attachments";
import type { ChatRender } from "./render";
import { readableError } from "./errors";

export type ChatPayload = Extract<WritEvent, { kind: "ai:chat" }>["payload"];

export type ChatStatus = "idle" | "thinking" | "streaming" | "done" | "stopped" | "error";

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
  /** The file a reply still arriving is writing a change to, while the change
   * is withheld from the text (ADR-040 section 9). Only a live reply has one. */
  writing?: string;
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
export interface Exchange {
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
  /** Which connection wrote each reply of this conversation. The file records
   * no such thing, so a turn read back off disk has none. */
  identityByTurn: Record<number, RequestIdentity>;
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
    identityByTurn: {},
    renderGeneration: new Map(),
    renderTimer: { handle: null },
  };
}

function newRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

/** The session state of every conversation, keyed by id. */
export function createEntries() {
  const [exchanges, setExchanges] = createSignal<Record<string, Exchange>>({});

  function entryOf(id: string | undefined | null): Exchange | undefined {
    return id ? exchanges()[id] : undefined;
  }

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

  function removeEntry(id: string): void {
    setExchanges((all) => {
      if (!(id in all)) return all;
      const next = { ...all };
      delete next[id];
      return next;
    });
  }

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

  return {
    exchanges,
    entryOf,
    ensureEntry,
    patchEntry,
    removeEntry,
    isLive,
    liveConversations,
    statusOf,
  };
}

export type ChatEntries = ReturnType<typeof createEntries>;

/** The pane's own state that a send reads and writes: the conversation on
 * screen, the composer, and a failure that belongs to no conversation. */
export interface ChatView {
  current: Accessor<ChatConversation | null>;
  setCurrent: Setter<ChatConversation | null>;
  draft: Accessor<string>;
  setDraft: (text: string) => void;
  editing: Accessor<number | null>;
  setEditing: (turn: number | null) => void;
  setPaneError: (message: string) => void;
}

/** What a send needs from the facade around it. */
export interface ExchangeHooks {
  /** The notes the send carries when the caller did not resolve them. */
  attachedOnDisk: () => Promise<Attachment[]>;
  /** Reads the list of conversations again. */
  refreshList: () => Promise<void>;
  /** Clears the view for a conversation the send just created. */
  clearView: () => void;
  /** Shows a failure that belongs to the pane. */
  failWith: (message: string) => void;
}

/**
 * Sending, retrying, stopping, and the frames a reply arrives in.
 */
export function createExchange(
  view: ChatView,
  entries: ChatEntries,
  render: ChatRender,
  hooks: ExchangeHooks,
) {
  // Held for as long as a send is between the draft and the status that says
  // the pane is busy. Retry takes it too, because it is the other way in.
  let sending = false;

  const currentEntry = (): Exchange | undefined => entries.entryOf(view.current()?.id);

  function isBusy(): boolean {
    const id = view.current()?.id;
    return id ? entries.isLive(id) : false;
  }

  /** The send Retry would repeat, from this session or from the file.
   *
   * Nothing is live after a relaunch, and a conversation whose last turn is a
   * person's is exactly a send whose reply never arrived, so it is offered
   * again from the file rather than from session state nothing kept. */
  function retryable(): { turn: number; text: string; paths: string[] } | null {
    const entry = currentEntry();
    if (entry?.lastSend) return entry.lastSend;
    const turns = view.current()?.turns ?? [];
    const last = turns[turns.length - 1];
    if (!last || last.role !== "user") return null;
    return {
      turn: turns.length - 1,
      text: last.content,
      paths: last.attachments.map((note) => note.path),
    };
  }

  /** Sends the draft. The caller has already cleared the blockers, so this
   * only names the conversation and hands the message over.
   *
   * `resolved` is the list the send dialog counted. It is passed in rather
   * than read again, so the bytes a person agreed to are the bytes that leave
   * the machine (R5 M5). */
  async function send(resolved?: Attachment[]) {
    const text = view.draft().trim();
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
    let conversation = view.current();
    if (!conversation) {
      try {
        conversation = await chatNew();
      } catch (error) {
        hooks.failWith(readableError(error));
        return;
      }
      const started = conversation;
      hooks.clearView();
      entries.ensureEntry(started.id);
      view.setCurrent(started);
      void hooks.refreshList();
    }

    // The paths come from the list the dialog counted, so one note is one path
    // in the request however each writer of the list spelled it.
    const notes = resolved ?? (await hooks.attachedOnDisk());
    const paths = notes.map((note) => note.path);
    const truncateTo = view.editing();
    const kept = truncateTo === null ? conversation.turns : conversation.turns.slice(0, truncateTo);
    if (truncateTo !== null) view.setCurrent({ ...conversation, turns: kept });
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
      if (entries.entryOf(id)?.requestId !== requestId) return;
      failSend(id, readableError(error));
      if (view.current()?.id === id) {
        view.setCurrent(conversation);
        view.setEditing(truncateTo);
        view.setDraft(text);
      }
    }
  }

  /** Sends the last message again, in place of the turn that failed. */
  async function retry() {
    const again = retryable();
    const conversation = view.current();
    // Retry takes the same latch Send holds, so the two cannot both open an
    // exchange on one conversation.
    if (!again || !conversation || isBusy() || sending) return;
    sending = true;
    const typed = view.draft();
    const id = conversation.id;
    try {
      view.setCurrent({ ...conversation, turns: conversation.turns.slice(0, again.turn) });
      const requestId = beginExchange(id, again.turn, again.text, again.paths);
      try {
        await chatSend(id, again.text, again.paths, again.turn, requestId);
      } catch (error) {
        // The retry was never accepted, so the turn it would have replaced is
        // still in the file and stays on screen, and anything half-typed is
        // still in the composer.
        if (entries.entryOf(id)?.requestId !== requestId) return;
        failSend(id, readableError(error));
        if (view.current()?.id === id) {
          view.setCurrent(conversation);
          view.setDraft(typed);
        }
      }
    } finally {
      sending = false;
    }
  }

  function failSend(id: string, message: string) {
    entries.patchEntry(id, {
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
    const entry = entries.ensureEntry(id);
    // A resend reuses the index the failed reply held, so its fragment goes
    // with it rather than sitting under the new one.
    const html = { ...entry.htmlByTurn };
    delete html[userTurn + 1];
    render.forgetTurn(id, userTurn + 1);
    const requestId = newRequestId();
    entries.patchEntry(id, {
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
    view.setPaneError("");
    view.setDraft("");
    view.setEditing(null);
    return requestId;
  }

  /** Stops a live reply, by the request id that is streaming rather than by
   * whatever the pane happens to show. The text already on screen stays, and
   * the reply it came from produced no proposal, so there is nothing to
   * apply. */
  function stop(id: string | undefined = view.current()?.id) {
    if (!id) return;
    const entry = entries.entryOf(id);
    if (!entry || !entries.isLive(id)) return;
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
    const entry = entries.entryOf(id);
    if (!entry || !entry.requestId || entry.requestId !== payload.request_id) return;
    if (payload.kind === "chunk") {
      if (entry.status !== "thinking" && entry.status !== "streaming") return;
      appendToReply(id, payload.text ?? "");
      entries.patchEntry(id, { status: "streaming" });
    } else if (payload.kind === "progress") {
      // The reply is writing a change the text withholds, so the pane says
      // which file rather than "Thinking" for as long as the change takes.
      if (entry.status !== "thinking" && entry.status !== "streaming") return;
      if (entry.reply) {
        entries.patchEntry(id, {
          reply: { ...entry.reply, writing: payload.text ?? "" },
          status: "streaming",
        });
      }
    } else if (payload.kind === "done") {
      if (entry.reply) {
        const identity = payload.identity ?? null;
        entries.patchEntry(id, {
          reply: {
            ...entry.reply,
            proposals: payload.proposals ?? [],
            dropped: payload.dropped ?? [],
            truncated: payload.truncated ?? false,
            identity,
            writing: undefined,
          },
          // Kept past the fold into the file, which records the turns and not
          // the connection that wrote them.
          identityByTurn: identity
            ? { ...entry.identityByTurn, [entry.reply.turn]: identity }
            : entry.identityByTurn,
        });
      }
      settle(id, "done");
    } else if (payload.kind === "stopped") {
      settle(id, "stopped");
    } else if (payload.kind === "error") {
      const typed = payload.error;
      entries.patchEntry(id, {
        errorMessage: typed?.message ?? payload.text ?? "The reply did not arrive.",
        errorKind: typed?.kind ?? "",
        errorIdentity: typed ? { provider: typed.provider, model: typed.model, host: "" } : null,
      });
      settle(id, "error");
    }
  }

  function appendToReply(id: string, text: string) {
    const entry = entries.entryOf(id);
    if (!entry?.reply) return;
    entries.patchEntry(id, {
      reply: { ...entry.reply, content: entry.reply.content + text, writing: undefined },
    });
    render.scheduleRender(id);
  }

  /** The stream is over, so the turns are in the file. The local fold keeps
   * the reply on screen at the indices Rust used; the reload that follows
   * replaces it with what the file actually holds.
   *
   * A conversation the pane is not showing has nothing to fold into: its file
   * already holds the reply, so the list is read again and its row says so. */
  function settle(id: string, ending: ChatStatus) {
    const entry = entries.entryOf(id);
    if (!entry) return;
    render.cancelScheduledRender(entry);
    const { user, reply } = entry;
    const wrote = reply !== null && reply.content.length > 0;
    const onScreen = view.current()?.id === id;
    const conversation = view.current();
    if (onScreen && user && conversation) {
      // Rust appends a reply only when it holds text, so an ending with
      // nothing shown leaves the user turn last in the file. A turn the file
      // already holds is not folded in a second time.
      const turns = [...conversation.turns];
      if (turns.length <= user.turn) turns.push(storedTurn(user));
      if (wrote && reply && turns.length <= reply.turn) turns.push(storedTurn(reply));
      view.setCurrent({ ...conversation, turns });
    }
    entries.patchEntry(id, { status: ending, requestId: "", user: null, reply: null });
    if (wrote && reply) void render.renderTurn(id, reply.turn, reply.content);
    if (onScreen) void reload(id);
    else void hooks.refreshList();
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
    if (view.current()?.id !== id) return;
    view.setCurrent(conversation);
    await render.renderStoredReplies(conversation);
    await hooks.refreshList();
  }

  return {
    currentEntry,
    isBusy,
    retryable,
    send,
    retry,
    stop,
    handleStreamEvent,
  };
}
