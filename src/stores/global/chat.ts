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
  /** The note's folder-relative key, where one has been read. Two paths with
   * the same key are the same note. */
  key?: string;
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

/** Whether a rebuilt turn shows anything the last one did not. A reload hands
 * back turns that are equal in new arrays, so this reads the fields rather
 * than the references. */
function sameMessage(held: Message, next: Message): boolean {
  return (
    held.role === next.role &&
    held.content === next.content &&
    held.html === next.html &&
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
  // Counts the chat openings the pane has to attach the note in front for: the
  // pane latches on it, so one open attaches once however often it re-renders.
  const [attachGeneration, setAttachGeneration] = createSignal(0);
  const [lastSend, setLastSend] = createSignal<{
    turn: number;
    text: string;
    paths: string[];
  } | null>(null);

  // A render is one round trip and a stream asks for many, so only the newest
  // result for a turn is painted: a slow one cannot overwrite a newer one.
  const renderGeneration = new Map<number, number>();
  // What each turn has been asked to render and what came back, keyed by
  // conversation and turn. A settle renders the finished reply and the reload
  // behind it walks the same turns, so without this one ending is two renders.
  // A null fragment is a render still in flight. What the pane shows is
  // checked as well as what was asked for, so a turn whose fragment was
  // replaced by another conversation's is rendered again rather than skipped.
  const renderAsked = new Map<string, { markdown: string; html: string | null }>();
  let renderTimer: ReturnType<typeof setTimeout> | null = null;
  // Held for as long as a send is between the draft and the status that says
  // the pane is busy.
  let sending = false;

  function toMessage(turn: ChatStoredTurn, index: number, html: string): Message {
    return {
      turn: index,
      role: turn.role,
      content: turn.content,
      html,
      attachments: turn.attachments,
      proposals: turn.proposals,
    };
  }

  /** The turns the file holds, with what each has been rendered to.
   *
   * A delta changes neither the conversation nor any settled turn's fragment,
   * so this returns the list it returned last: the settled half of a long
   * conversation is not rebuilt, and not re-compared, once per token. The
   * comparison is kept for the reload behind a settle, which hands back equal
   * turns in new objects. */
  // The turns as the pane last saw them, pending tail included: a settle moves
  // a turn from the tail into the file, and the object it was shown as is the
  // one to keep.
  let lastShown: Message[] = [];

  const settled = createMemo<{
    source: ChatConversation | null;
    html: string[];
    list: Message[];
  }>((before) => {
    const conversation = current();
    const rendered = htmlByTurn();
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
    const held = new Map(lastShown.map((message) => [message.turn, message]));
    const list = shown.map((message) => {
      const kept = held.get(message.turn);
      return kept && sameMessage(kept, message) ? kept : message;
    });
    return { source: conversation, html, list };
  });

  /** The turns of a send the file does not hold yet: the person's, and the
   * reply as far as it has arrived. This is the only part a delta rebuilds. */
  const pending = createMemo<Message[]>(() => {
    const user = pendingUser();
    const reply = pendingReply();
    const tail: Message[] = [];
    if (user) tail.push(user);
    if (reply) tail.push({ ...reply, html: htmlByTurn()[reply.turn] ?? "" });
    return tail;
  });

  /** The turns on screen, oldest first. A turn that says what it said keeps
   * its object, so its element is left alone and the copy button a person just
   * pressed is still the element that was pressed. */
  const messages = createMemo<Message[]>(() => {
    const shown = [...settled().list, ...pending()];
    lastShown = shown;
    return shown;
  });

  function isBusy(): boolean {
    return status() === "thinking" || status() === "streaming";
  }

  /** Whether the pane holds a message it could send again. Retry re-sends the
   * last one, so an error with nothing behind it is a message and no button. */
  function canRetry(): boolean {
    return lastSend() !== null;
  }

  /** Shows a failure that nothing can be sent again for: opening, renaming and
   * deleting a conversation all end here. */
  function failWith(message: string) {
    setLastSend(null);
    setStatus("error");
    setErrorMessage(message);
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
    renderAsked.clear();
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
    // Only the provider's own list is offered here; the table's suggestions
    // are marked as such in Settings and are not an inventory.
    setLiveModels(listed.source === "live" ? listed.models : []);
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
      failWith(readableError(error));
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
    // The notes belong to the chat they were attached in, so a new one starts
    // with the note in front and nothing the last chat carried.
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

  async function remove(id: string) {
    try {
      await chatDelete(id);
    } catch (error) {
      failWith(readableError(error));
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
    // The status says nothing until the notes have been read off disk, so the
    // latch is what a second Send before that meets.
    if (!text || isBusy() || sending) return;
    sending = true;
    try {
      await sendTyped(text);
    } finally {
      sending = false;
    }
  }

  async function sendTyped(text: string) {
    let conversation = current();
    if (!conversation) {
      try {
        conversation = await chatNew();
      } catch (error) {
        failWith(readableError(error));
        return;
      }
      const started = conversation;
      reset();
      setCurrent(started);
      void refreshList();
    }

    // The paths come from the list the dialog counted, so one note is one path
    // in the request however each writer of the list spelled it.
    const paths = (await attachedOnDisk().catch(() => attachments())).map((note) => note.path);
    const truncateTo = editing();
    const kept = truncateTo === null ? conversation.turns : conversation.turns.slice(0, truncateTo);
    if (truncateTo !== null) setCurrent({ ...conversation, turns: kept });
    beginExchange(kept.length, text, paths);

    try {
      await chatSend(conversation.id, text, paths, truncateTo ?? undefined);
    } catch (error) {
      // A send that was never accepted leaves nothing on screen and the words
      // back in the composer, to send again or edit. The turns an edit would
      // have replaced are still in the file, so they are still shown and the
      // next send still replaces them rather than being added after them.
      setPendingUser(null);
      setPendingReply(null);
      setCurrent(conversation);
      setEditing(truncateTo);
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
    const typed = draft();
    setCurrent({ ...conversation, turns: conversation.turns.slice(0, again.turn) });
    beginExchange(again.turn, again.text, again.paths);
    try {
      await chatSend(conversation.id, again.text, again.paths, again.turn);
    } catch (error) {
      // The retry was never accepted, so the turn it would have replaced is
      // still in the file and stays on screen, and anything half-typed is
      // still in the composer.
      setPendingUser(null);
      setPendingReply(null);
      setCurrent(conversation);
      setDraft(typed);
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
    const id = current()?.id;
    if (id) renderAsked.delete(`${id}:${userTurn + 1}`);
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
      if (!isBusy()) return;
      settle("stopped");
    } else if (payload.kind === "error") {
      if (!isBusy()) return;
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

  /** Drops a record of a render that painted nothing, and only that record: a
   * newer render for the same turn keeps what it wrote. */
  function forgetRender(asked: string, markdown: string) {
    const held = renderAsked.get(asked);
    if (held?.markdown === markdown && held.html === null) renderAsked.delete(asked);
  }

  /** Renders one reply to a fragment. A result a newer render has already
   * overtaken is dropped rather than painted. */
  async function renderTurn(id: string, turn: number, markdown: string) {
    const asked = `${id}:${turn}`;
    const held = renderAsked.get(asked);
    if (held?.markdown === markdown && (held.html === null || htmlByTurn()[turn] === held.html)) {
      return;
    }
    renderAsked.set(asked, { markdown, html: null });
    const generation = (renderGeneration.get(turn) ?? 0) + 1;
    renderGeneration.set(turn, generation);
    let html: string;
    try {
      html = await chatRenderReply(markdown);
    } catch {
      // Nothing was painted, so the next ask for this text is a fresh one.
      forgetRender(asked, markdown);
      return;
    }
    if (renderGeneration.get(turn) !== generation || current()?.id !== id) {
      forgetRender(asked, markdown);
      return;
    }
    renderAsked.set(asked, { markdown, html });
    setHtmlByTurn((shown) => ({ ...shown, [turn]: html }));
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

  /** The attached notes with the sizes the files hold now, one note per note.
   *
   * A tab records a note's size when it reads it, and another program can
   * rewrite the file after that. The dialog asking to send it must state the
   * bytes the send will carry, so it asks disk rather than the tab. Two
   * spellings of one note are one line in that sentence and one path in the
   * request, which is why the send reads this list rather than the chips. */
  async function attachedOnDisk(): Promise<Attachment[]> {
    const held = attachments();
    const sizes = await chatAttachedSizes(held.map((note) => note.path));
    // Keyed by the path that was asked about, which is the spelling these
    // attachments hold. The command's own folder-relative key names the same
    // note in a different shape and would miss every row.
    const byPath = new Map(sizes.map((note) => [note.path, note]));
    const counted = new Set<string>();
    const shown: Attachment[] = [];
    for (const note of held) {
      const found = byPath.get(note.path);
      const key = found?.key ?? note.key;
      if (key !== undefined) {
        if (counted.has(key)) continue;
        counted.add(key);
      }
      shown.push({ ...note, bytes: found?.bytes ?? note.bytes });
    }
    return shown;
  }

  return {
    conversations,
    current,
    messages,
    liveModels,
    loadModels,
    copyCode,
    attachments,
    attachGeneration,
    status,
    errorMessage,
    canRetry,
    draft,
    setDraft,
    editing,
    refusalFor,
    attach,
    attachAll,
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
    attachedOnDisk,
  };
}

function readableError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "The reply did not arrive.";
}

export type ChatStore = ReturnType<typeof createChatStore>;
export const chatStore = createRoot(createChatStore);
