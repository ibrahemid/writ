import { chatRenderReply, type ChatConversation } from "../../../services/tauri";
import type { ChatEntries, Exchange } from "./exchange";

/** How often a live reply is re-rendered while it streams (ADR-040 section 9). */
export const RENDER_THROTTLE_MS = 120;

/**
 * Turns a reply's markdown into the fragment the pane shows, at most once per
 * text and at most every [`RENDER_THROTTLE_MS`] while it streams.
 *
 * A fragment lands in the conversation that asked for it, whether or not the
 * pane still shows that conversation, and a fragment a newer render has
 * already overtaken is dropped rather than painted.
 */
export function createRender(entries: ChatEntries) {
  // What each turn has been asked to render and what came back, keyed by
  // conversation and turn. A settle renders the finished reply and the reload
  // behind it walks the same turns, so without this one ending is two renders.
  // A null fragment is a render still in flight.
  const renderAsked = new Map<string, { markdown: string; html: string | null }>();

  function scheduleRender(id: string) {
    const entry = entries.entryOf(id);
    if (!entry || entry.renderTimer.handle !== null) return;
    entry.renderTimer.handle = setTimeout(() => {
      entry.renderTimer.handle = null;
      const reply = entries.entryOf(id)?.reply;
      if (!reply) return;
      void renderTurn(id, reply.turn, reply.content);
    }, RENDER_THROTTLE_MS);
  }

  function cancelScheduledRender(entry: Exchange) {
    if (entry.renderTimer.handle === null) return;
    clearTimeout(entry.renderTimer.handle);
    entry.renderTimer.handle = null;
  }

  /** Forgets every render of one conversation, which is being dropped. */
  function forgetConversation(id: string) {
    for (const key of [...renderAsked.keys()]) {
      if (key.startsWith(`${id}:`)) renderAsked.delete(key);
    }
  }

  /** Forgets one turn's render, for a resend that reuses its index. */
  function forgetTurn(id: string, turn: number) {
    renderAsked.delete(`${id}:${turn}`);
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
    const entry = entries.entryOf(id);
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
    const still = entries.entryOf(id);
    if (!still || still.renderGeneration.get(turn) !== generation) {
      forgetRender(asked, markdown);
      return;
    }
    renderAsked.set(asked, { markdown, html });
    entries.patchEntry(id, { htmlByTurn: { ...still.htmlByTurn, [turn]: html } });
  }

  async function renderStoredReplies(conversation: ChatConversation) {
    for (const [index, turn] of conversation.turns.entries()) {
      if (turn.role !== "assistant" || turn.content.length === 0) continue;
      await renderTurn(conversation.id, index, turn.content);
    }
  }

  return {
    scheduleRender,
    cancelScheduledRender,
    forgetConversation,
    forgetTurn,
    renderTurn,
    renderStoredReplies,
  };
}

export type ChatRender = ReturnType<typeof createRender>;
