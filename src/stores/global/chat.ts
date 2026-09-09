import { createSignal, createRoot } from "solid-js";
import {
  chatState,
  chatSend,
  chatCancel,
  chatApplyProposal,
  chatDiscardProposal,
  type ChatEndpointState,
  type ChatAttachedNote,
  type ChatProposal,
  type ChatProposalOutcome,
  type ChatTurn,
} from "../../services/tauri";
import type { WritEvent } from "../../types/events";

export type { ChatAttachedNote, ChatEndpointState, ChatProposal };

type ChatPayload = Extract<WritEvent, { kind: "ai:chat" }>["payload"];

export type ChatStatus = "idle" | "streaming" | "done" | "error";

/** A note the conversation carries, as the pane lists it. */
export interface Attachment {
  /** Absolute path, which is what the send names. */
  path: string;
  /** The file's name, which is what the row shows. */
  name: string;
  /** The note's size on disk. */
  bytes: number;
}

/** One turn on screen. A reply carries the proposals it produced. */
export interface Message {
  role: "user" | "assistant";
  content: string;
  proposals: ChatProposal[];
  /** What the request carried, so a proposal can be read beside the text the
   * model was given rather than beside whatever the file holds now. */
  context: ChatAttachedNote[];
  /** Proposals already applied or discarded, by path, so a decided one stops
   * offering its two buttons without leaving the reply that made it. */
  decided: Record<string, "applied" | "discarded" | "refused">;
  /** Why applying was refused, for the proposal it was refused for. */
  refusal: Record<string, string>;
}

/** How the conversation reads a note's size for the send dialog. */
export function totalBytes(attachments: readonly Attachment[]): number {
  return attachments.reduce((sum, note) => sum + note.bytes, 0);
}

// Singleton state — Writ is single-window. One conversation is live at a time,
// and the pane is the only thing that shows it.
function createChatStore() {
  const [messages, setMessages] = createSignal<Message[]>([]);
  const [attachments, setAttachments] = createSignal<Attachment[]>([]);
  const [status, setStatus] = createSignal<ChatStatus>("idle");
  const [errorMessage, setErrorMessage] = createSignal("");
  const [draft, setDraft] = createSignal("");
  const [conversationId, setConversationId] = createSignal("");

  function reset() {
    setMessages([]);
    setStatus("idle");
    setErrorMessage("");
    setDraft("");
    setConversationId("");
  }

  /** Adds a note to what the request carries. Attaching is a user's action and
   * nothing else adds one, so the list the pane shows is the whole of what
   * leaves the machine (ADR-031 rule 2.5). */
  function attach(note: Attachment) {
    setAttachments((current) =>
      current.some((held) => held.path === note.path) ? current : [...current, note],
    );
  }

  function detach(path: string) {
    setAttachments((current) => current.filter((note) => note.path !== path));
  }

  function isAttached(path: string): boolean {
    return attachments().some((note) => note.path === path);
  }

  /** Sends the draft. The caller has already cleared the blockers, so this
   * only builds the turn list and hands it over. */
  async function send() {
    const text = draft().trim();
    if (!text || status() === "streaming") return;
    const id = newConversationId();
    const turns: ChatTurn[] = [
      ...messages().map((message) => ({ role: message.role, content: message.content })),
      { role: "user" as const, content: text },
    ];
    setMessages((current) => [...current, message("user", text), message("assistant", "")]);
    setDraft("");
    setErrorMessage("");
    setStatus("streaming");
    setConversationId(id);
    try {
      const accepted = await chatSend(
        id,
        turns,
        attachments().map((note) => note.path),
      );
      if (conversationId() === id) setContext(accepted.attached);
    } catch (error) {
      if (conversationId() !== id) return;
      setStatus("error");
      setErrorMessage(readableError(error));
    }
  }

  /** Stops a live reply. The text already on screen stays, and the reply it
   * came from produced no proposal, so there is nothing to apply. */
  function stop() {
    const id = conversationId();
    if (!id || status() !== "streaming") return;
    void chatCancel(id);
    setStatus("idle");
  }

  function handleStreamEvent(payload: ChatPayload) {
    if (!payload || payload.conversation_id !== conversationId()) return;
    if (payload.kind === "chunk") {
      if (status() !== "streaming") return;
      appendToReply(payload.text ?? "");
    } else if (payload.kind === "done") {
      if (status() !== "streaming") return;
      setProposals(payload.proposals ?? []);
      setStatus("done");
    } else if (payload.kind === "error") {
      setStatus("error");
      setErrorMessage(payload.text ?? "The reply did not arrive.");
    }
  }

  function appendToReply(text: string) {
    setMessages((current) => {
      const next = [...current];
      const last = next[next.length - 1];
      if (!last || last.role !== "assistant") return current;
      next[next.length - 1] = { ...last, content: last.content + text };
      return next;
    });
  }

  function setContext(context: ChatAttachedNote[]) {
    setMessages((current) => {
      const next = [...current];
      const last = next[next.length - 1];
      if (!last || last.role !== "assistant") return current;
      next[next.length - 1] = { ...last, context };
      return next;
    });
  }

  function setProposals(proposals: ChatProposal[]) {
    setMessages((current) => {
      const next = [...current];
      const last = next[next.length - 1];
      if (!last || last.role !== "assistant") return current;
      next[next.length - 1] = { ...last, proposals };
      return next;
    });
  }

  function decide(path: string, verdict: Message["decided"][string], why = "") {
    setMessages((current) =>
      current.map((message) =>
        message.proposals.some((proposal) => proposal.path === path)
          ? {
              ...message,
              decided: { ...message.decided, [path]: verdict },
              refusal: why ? { ...message.refusal, [path]: why } : message.refusal,
            }
          : message,
      ),
    );
  }

  /** Writes one proposal, through the guarded facade and nothing else.
   *
   * A note that changed since the proposal was made is refused there, and the
   * refusal is what the row then shows: the proposed text is on disk beside
   * the note, and the note is as its last writer left it. */
  async function apply(proposal: ChatProposal): Promise<ChatProposalOutcome | null> {
    try {
      const outcome = await chatApplyProposal(
        proposal.path,
        proposal.new_content,
        proposal.before_hash,
      );
      decide(proposal.path, "applied");
      return outcome;
    } catch (error) {
      decide(proposal.path, "refused", readableError(error));
      return null;
    }
  }

  async function discard(proposal: ChatProposal) {
    decide(proposal.path, "discarded");
    await chatDiscardProposal(proposal.path).catch(() => undefined);
  }

  return {
    messages,
    attachments,
    status,
    errorMessage,
    draft,
    setDraft,
    attach,
    detach,
    isAttached,
    send,
    stop,
    apply,
    discard,
    reset,
    handleStreamEvent,
    /** Where the chat endpoint points and what it still needs. */
    endpointState: (): Promise<ChatEndpointState> => chatState(),
  };
}

function message(role: Message["role"], content: string): Message {
  return { role, content, proposals: [], context: [], decided: {}, refusal: {} };
}

function newConversationId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function readableError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "The reply did not arrive.";
}

export type ChatStore = ReturnType<typeof createChatStore>;
export const chatStore = createRoot(createChatStore);
