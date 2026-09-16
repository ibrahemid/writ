import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// What a delta is allowed to touch. A stream arrives tens of times a second,
// so a token that changes nothing on screen must leave the element it lands in
// exactly as it was.

const mocks = vi.hoisted(() => ({
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatRenderReply: vi.fn(),
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatAttachedSizes: vi.fn(),
  writeClipboardText: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  chatState: vi.fn(),
  openExternalUrl: vi.fn(),
  classifyExternalUrl: vi.fn(),
  noteNameCandidates: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: mocks.chatList,
  chatOpen: mocks.chatOpen,
  chatNew: mocks.chatNew,
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: mocks.chatRenderReply,
  chatSend: mocks.chatSend,
  chatStop: mocks.chatStop,
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
}));

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: mocks.writeClipboardText,
}));

import ChatTranscript from "../../components/Chat/ChatTranscript";
import { chatStore } from "../../stores/global/chat";
import type { ChatConversation } from "../../services/tauri";

function conversation(id: string): ChatConversation {
  return {
    id,
    title: "A chat",
    created_at: "2026-09-15T10:00:00+00:00",
    updated_at: "2026-09-15T10:00:00+00:00",
    provider: "anthropic",
    model: "a-model",
    turns: [],
  };
}

beforeEach(() => {
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.chatList.mockResolvedValue([]);
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatNew.mockResolvedValue(conversation("c1"));
  mocks.chatSend.mockResolvedValue({ conversation_id: "c1", attached: [] });
  mocks.chatAttachedSizes.mockResolvedValue([]);
  mocks.writeClipboardText.mockResolvedValue(undefined);
  chatStore.reset();
});

afterEach(() => {
  cleanup();
});

function chunk(text: string) {
  chatStore.handleStreamEvent({ conversation_id: "c1", request_id: "r-1", kind: "chunk", text });
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

/** Waits for the next throttled render to reach the pane. */
async function nextRender() {
  const before = mocks.chatRenderReply.mock.calls.length;
  await waitFor(() => expect(mocks.chatRenderReply.mock.calls.length).toBeGreaterThan(before));
  await flush();
}

async function openTranscript() {
  const { container } = render(() => <ChatTranscript />);
  chatStore.setDraft("what does it argue");
  await chatStore.send();
  return container;
}

describe("a streaming reply", () => {
  it("keeps the streaming turn's element across deltas", async () => {
    const container = await openTranscript();

    chunk("it argues ");
    await nextRender();
    const reply = container.querySelectorAll(".chat-turn")[1];
    expect(reply).toBeDefined();

    for (const text of ["this ", "and ", "that"]) chunk(text);
    await nextRender();

    expect(container.querySelectorAll(".chat-turn")[1]).toBe(reply);
  });

  // A real reply is not one paragraph: it opens a fence the renderer withholds
  // until it closes, carries a block wider than the column, and is followed by
  // another turn.
  it("keeps a long reply readable and copyable while a second turn follows", async () => {
    const code = Array.from({ length: 40 }, (_, line) => `let counter_${line} = ${line};`).join(
      "\n",
    );
    mocks.chatRenderReply.mockImplementation(async (text: string) => {
      const fences = (text.match(/```/g) ?? []).length;
      if (fences < 2) return "<p>Here is how it stands.</p>";
      return (
        "<p>Here is how it stands.</p>" +
        `<pre><code>${code}</code></pre>` +
        "<ul><li>the first</li><li>the second</li></ul>"
      );
    });
    const container = await openTranscript();

    chunk("Here is how it stands.\n\n```rust\n");
    await nextRender();
    const reply = container.querySelectorAll(".chat-turn")[1];
    expect(reply.querySelector("pre")).toBeNull();

    for (const line of code.split("\n")) {
      chunk(`${line}\n`);
      if (line.endsWith("9;")) await nextRender();
    }
    chunk("```\n\n- the first\n- the second");
    await nextRender();

    // Same element throughout, with the block beside its button rather than
    // around it, and the list the fence was holding back.
    expect(container.querySelectorAll(".chat-turn")[1]).toBe(reply);
    const box = reply.querySelector(".chat-code") as HTMLElement;
    const button = box.querySelector(".chat-code-copy") as HTMLElement;
    expect(box.querySelector("pre")).not.toBeNull();
    expect(button.closest("pre")).toBeNull();
    expect(reply.querySelectorAll("li")).toHaveLength(2);

    fireEvent.click(button);
    await waitFor(() => expect(mocks.writeClipboardText).toHaveBeenCalledWith(code));

    // The turn is settled, read back from the file, and a second exchange runs
    // on top of it.
    mocks.chatOpen.mockResolvedValue({
      ...conversation("c1"),
      turns: [
        { role: "user" as const, content: "how does it stand", attachments: [], proposals: [] },
        {
          role: "assistant" as const,
          content: `Here is how it stands.\n\n\`\`\`rust\n${code}\n\`\`\`\n\n- the first\n- the second`,
          attachments: [],
          proposals: [],
        },
      ],
    });
    chatStore.handleStreamEvent({ conversation_id: "c1", request_id: "r-1", kind: "done", proposals: [] });
    await flush();
    chatStore.setDraft("and then");
    await chatStore.send();
    chunk("The second answer.");
    await nextRender();

    expect(container.querySelectorAll(".chat-turn")[1]).toBe(reply);
    expect(button.isConnected).toBe(true);
    expect(container.querySelectorAll(".chat-turn")).toHaveLength(4);
  });

  it("re-writes the reply fragment only when the rendered HTML changes", async () => {
    const container = await openTranscript();
    chunk("it argues ");
    await nextRender();
    const scroller = container.querySelector(".chat-transcript") as HTMLElement;
    await waitFor(() => expect(scroller.querySelector(".chat-reply")).not.toBeNull());

    // The callback drains what the observer holds, so both it and the pending
    // records are counted: an await between deltas otherwise loses them.
    let records = 0;
    const observer = new MutationObserver((batch) => {
      records += batch.length;
    });
    observer.observe(scroller, { childList: true, subtree: true, characterData: true });

    // Deltas inside one throttle window render nothing, so they must move
    // nothing: the fragment on screen already says what they say.
    for (const text of ["this ", "and ", "that"]) chunk(text);
    records += observer.takeRecords().length;
    expect(records).toBe(0);

    // Twenty deltas across four windows: the churn is bounded by the renders,
    // not by the tokens.
    const rendersBefore = mocks.chatRenderReply.mock.calls.length;
    for (let window = 0; window < 4; window += 1) {
      for (let delta = 0; delta < 5; delta += 1) chunk(`w${window}d${delta} `);
      await nextRender();
    }
    records += observer.takeRecords().length;
    const renders = mocks.chatRenderReply.mock.calls.length - rendersBefore;

    observer.disconnect();
    expect(renders).toBeLessThanOrEqual(5);
    expect(records).toBeLessThanOrEqual(renders * 3);
  });
});
