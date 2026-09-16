import { describe, it, expect, vi, beforeEach } from "vitest";

// One conversation is on screen and any number of them can be streaming. What
// this pins down is the routing that keeps those two facts apart: which
// exchange a frame belongs to, which request a stop names, and what a refusal
// is allowed to clear.

const mocks = vi.hoisted(() => ({
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: vi.fn(),
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
  chatAttachedSizes: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: mocks.chatList,
  chatOpen: mocks.chatOpen,
  chatNew: mocks.chatNew,
  chatRename: mocks.chatRename,
  chatDelete: mocks.chatDelete,
  chatRenderReply: mocks.chatRenderReply,
  chatSend: mocks.chatSend,
  chatStop: mocks.chatStop,
  chatApplyProposal: mocks.chatApplyProposal,
  chatDiscardProposal: mocks.chatDiscardProposal,
}));

import { chatStore } from "../../stores/global/chat";
import type { ChatConversation } from "../../services/tauri";

/** What each conversation's file holds, which is what `chat_open` answers. */
const files: Record<string, ChatConversation["turns"]> = {};

function conversation(id: string): ChatConversation {
  return {
    id,
    title: id,
    created_at: "2026-09-16T10:00:00+00:00",
    updated_at: "2026-09-16T10:00:00+00:00",
    provider: "ollama",
    model: "a-model",
    turns: files[id] ?? [],
  };
}

function summary(id: string) {
  return {
    id,
    title: id,
    created_at: "2026-09-16T10:00:00+00:00",
    updated_at: "2026-09-16T10:00:00+00:00",
    turns: (files[id] ?? []).length,
  };
}

function userTurn(content: string) {
  return { role: "user" as const, content, attachments: [], proposals: [] };
}

function replyTurn(content: string) {
  return { role: "assistant" as const, content, attachments: [], proposals: [] };
}

/** The id the last send on that conversation was minted with. */
function rid(id: string): string {
  const calls = mocks.chatSend.mock.calls as unknown[][];
  for (let index = calls.length - 1; index >= 0; index -= 1) {
    if (calls[index][0] === id) return calls[index][4] as string;
  }
  return "no-such-request";
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

/** Sends one message into the conversation on screen, as Rust would: the user
 * turn is in the file from the moment the send is accepted. */
async function sendInto(id: string, text: string) {
  chatStore.setDraft(text);
  await chatStore.send();
  files[id] = [...(files[id] ?? []), userTurn(text)];
}

beforeEach(async () => {
  for (const key of Object.keys(files)) delete files[key];
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.chatList.mockImplementation(async () => Object.keys(files).map(summary));
  mocks.chatOpen.mockImplementation(async (id: string) => conversation(id));
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatSend.mockResolvedValue({ conversation_id: "c1", request_id: "r", attached: [] });
  mocks.chatStop.mockResolvedValue(undefined);
  mocks.chatDelete.mockResolvedValue(undefined);
  mocks.chatAttachedSizes.mockResolvedValue([]);
  chatStore.reset();
  for (const note of chatStore.attachments()) chatStore.detach(note.path);
  files.c1 = [];
  files.c2 = [];
  await chatStore.openPane();
  await chatStore.open("c1");
});

describe("two conversations, one pane", () => {
  it("switching conversations keeps the streaming exchange alive", async () => {
    await sendInto("c1", "what does it argue");
    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: rid("c1"),
      kind: "chunk",
      text: "half",
    });

    await chatStore.open("c2");
    expect(chatStore.status()).toBe("idle");
    expect(chatStore.isLive("c1")).toBe(true);

    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: rid("c1"),
      kind: "chunk",
      text: "way",
    });

    await chatStore.open("c1");
    expect(chatStore.status()).toBe("streaming");
    expect(chatStore.messages().map((turn) => turn.content)).toEqual([
      "what does it argue",
      "halfway",
    ]);
  });

  it("a late frame for another conversation never touches the pane", async () => {
    await sendInto("c1", "what does it argue");
    await chatStore.open("c2");
    const before = chatStore.messages();

    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: rid("c1"),
      kind: "chunk",
      text: "not for this pane",
    });

    expect(chatStore.messages()).toEqual(before);
    expect(chatStore.status()).toBe("idle");
    expect(chatStore.errorMessage()).toBe("");
  });

  it("stop cancels the request that is streaming, not the one on screen", async () => {
    await sendInto("c1", "what does it argue");
    const streaming = rid("c1");
    await chatStore.open("c2");

    chatStore.stop("c1");

    expect(mocks.chatStop).toHaveBeenCalledWith("c1", streaming);
    expect(mocks.chatStop).toHaveBeenCalledTimes(1);
  });

  it("a frame with a stale request id is dropped", async () => {
    await sendInto("c1", "what does it argue");

    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: "a-request-that-ended",
      kind: "chunk",
      text: "from the send before",
    });

    expect(chatStore.messages()[1].content).toBe("");
    expect(chatStore.status()).toBe("thinking");
  });

  it("removing a streaming conversation cancels it first", async () => {
    await sendInto("c1", "what does it argue");
    const streaming = rid("c1");
    const order: string[] = [];
    mocks.chatStop.mockImplementation(async () => {
      order.push("stop");
    });
    mocks.chatDelete.mockImplementation(async () => {
      order.push("delete");
    });

    await chatStore.remove("c1");

    expect(order).toEqual(["stop", "delete"]);
    expect(mocks.chatStop).toHaveBeenCalledWith("c1", streaming);
    expect(chatStore.current()).toBeNull();
  });

  it("a rejected send clears only its own exchange", async () => {
    await sendInto("c1", "what does it argue");
    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: rid("c1"),
      kind: "chunk",
      text: "still arriving",
    });

    await chatStore.open("c2");
    mocks.chatSend.mockRejectedValueOnce("This chat is full. Start a new chat to continue.");
    chatStore.setDraft("a second question");
    await chatStore.send();

    expect(chatStore.status()).toBe("error");
    expect(chatStore.isLive("c1")).toBe(true);

    await chatStore.open("c1");
    expect(chatStore.status()).toBe("streaming");
    expect(chatStore.messages()[1].content).toBe("still arriving");
  });

  // A failure is the last thing that happened to that conversation, so coming
  // back to it says so rather than showing a clean pane over a message that
  // never landed.
  it("a conversation whose send failed still says so when it is reopened", async () => {
    mocks.chatSend.mockRejectedValueOnce("The model did not answer.");
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    expect(chatStore.status()).toBe("error");

    await chatStore.open("c2");
    expect(chatStore.status()).toBe("idle");
    expect(chatStore.errorMessage()).toBe("");

    await chatStore.open("c1");

    expect(chatStore.status()).toBe("error");
    expect(chatStore.errorMessage()).toBe("The model did not answer.");
    expect(chatStore.canRetry()).toBe(true);
  });

  it("a partial reply from a stopped exchange survives reopening", async () => {
    await sendInto("c1", "what does it argue");
    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: rid("c1"),
      kind: "chunk",
      text: "half an ans",
    });
    await chatStore.open("c2");

    // Rust records what arrived before it emits the ending, so the file holds
    // the partial reply by the time the frame lands.
    files.c1 = [...files.c1, replyTurn("half an ans")];
    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: rid("c1"),
      kind: "stopped",
    });
    await flush();

    expect(chatStore.statusOf("c1")).toBe("stopped");
    await chatStore.open("c1");
    expect(chatStore.messages().map((turn) => turn.content)).toEqual([
      "what does it argue",
      "half an ans",
    ]);
  });
});

describe("one conversation, two ways in", () => {
  it("retry cannot run while a send is in flight", async () => {
    files.c1 = [userTurn("what does it argue")];
    await chatStore.open("c1");
    expect(chatStore.canRetry()).toBe(true);

    let release: () => void = () => {};
    mocks.chatSend.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = () => resolve({ conversation_id: "c1", request_id: "r", attached: [] });
        }),
    );
    chatStore.setDraft("a second question");
    const inFlight = chatStore.send();
    await flush();

    await chatStore.retry();
    expect(mocks.chatSend).toHaveBeenCalledTimes(1);

    release();
    await inFlight;
  });

  // Nothing is live after a relaunch, so a conversation ending on a person's
  // turn is the only record that a reply never arrived.
  it("offers Retry from a trailing user turn after a relaunch", async () => {
    files.c1 = [userTurn("what does it argue")];
    await chatStore.open("c1");

    expect(chatStore.canRetry()).toBe(true);
    await chatStore.retry();

    expect(mocks.chatSend).toHaveBeenCalledWith(
      "c1",
      "what does it argue",
      [],
      0,
      expect.any(String),
    );
  });
});
