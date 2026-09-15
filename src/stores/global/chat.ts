import { createSignal, createRoot } from "solid-js";
import {
  chatState,
  chatAttachedSizes,
  chatNew,
  chatSend,
  chatCancel,
  chatApplyProposal,
  chatDiscardProposal,
  type ChatEndpointState,
  type ChatAttachedNote,
  type ChatProposal,
  type ChatProposalOutcome,
} from "../../services/tauri";
import type { WritEvent } from "../../types/events";

export type { ChatAttachedNote, ChatEndpointState, ChatProposal };

type ChatPayload = Extract<WritEvent, { kind: "ai:chat" }>["payload"];

export type ChatStatus = "idle" | "streaming" | "done" | "stopped" | "error";

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
   * only names the conversation and hands the message over.
   *
   * The turns live in a file Rust owns, so the send names the conversation
   * rather than replaying it: a conversation that does not exist yet is
   * created here, on the first send. */
  async function send() {
    const text = draft().trim();
    if (!text || status() === "streaming") return;
    let id = conversationId();
    setMessages((current) => [...current, message("user", text), message("assistant", "")]);
    setDraft("");
    setErrorMessage("");
    setStatus("streaming");
    try {
      if (!id) {
        id = (await chatNew()).id;
        setConversationId(id);
      }
      const accepted = await chatSend(
        id,
        text,
        attachments().map((note) => note.path),
      );
      if (conversationId() === id) setContext(accepted.attached);
    } catch (error) {
      if (id && conversationId() !== id) return;
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
    } else if (payload.kind === "stopped") {
      setStatus("stopped");
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
        conversationId(),
        turnOf(proposal),
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
    await chatDiscardProposal(conversationId(), turnOf(proposal), proposal.path).catch(
      () => undefined,
    );
  }

  /** Which turn of the stored conversation offered a proposal. */
  function turnOf(proposal: ChatProposal): number {
    return messages().findIndex((held) =>
      held.proposals.some((offered) => offered.path === proposal.path),
    );
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
    /** The attached notes with the sizes the files hold now.
     *
     * A tab records a note's size when it reads it, and another program can
     * rewrite the file after that. The dialog asking to send it must state the
     * bytes the send will carry, so it asks disk rather than the tab. */
    async attachedOnDisk(): Promise<Attachment[]> {
      const current = attachments();
      const sizes = await chatAttachedSizes(current.map((note) => note.path));
      // Keyed by the path that was asked about, which is the absolute one
      // these attachments hold. The command's own folder-relative key names
      // the same note in a different shape and would miss every row.
      const byPath = new Map(sizes.map((note) => [note.path, note.bytes]));
      return current.map((note) => ({ ...note, bytes: byPath.get(note.path) ?? note.bytes }));
    },
  };
}

function message(role: Message["role"], content: string): Message {
  return { role, content, proposals: [], context: [], decided: {}, refusal: {} };
}

function readableError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "The reply did not arrive.";
}

export type ChatStore = ReturnType<typeof createChatStore>;
export const chatStore = createRoot(createChatStore);
