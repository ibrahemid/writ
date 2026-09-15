import { describe, it, expect, vi, beforeEach } from "vitest";

// The conversation is a file Rust owns and the store is a view of it. What
// this pins down is the arithmetic that keeps the two agreeing: which turn
// index a proposal is applied at, what a retry cuts off, and what an edit
// replaces.

const mocks = vi.hoisted(() => ({
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: vi.fn(),
  chatSend: vi.fn(),
  chatCancel: vi.fn(),
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
  chatCancel: mocks.chatCancel,
  chatApplyProposal: mocks.chatApplyProposal,
  chatDiscardProposal: mocks.chatDiscardProposal,
}));

import { chatStore, RENDER_THROTTLE_MS } from "../../stores/global/chat";
import type { ChatConversation, ChatProposal } from "../../services/tauri";

function summary(id: string, title: string, updated: string) {
  return { id, title, created_at: updated, updated_at: updated, turns: 2 };
}

function conversation(id: string, turns: ChatConversation["turns"] = []): ChatConversation {
  return {
    id,
    title: "A chat",
    created_at: "2026-09-15T10:00:00+00:00",
    updated_at: "2026-09-15T10:00:00+00:00",
    provider: "anthropic",
    model: "a-model",
    turns,
  };
}

function userTurn(content: string, paths: string[] = []) {
  return {
    role: "user" as const,
    content,
    attachments: paths.map((path) => ({ path, bytes: 10, hash: "h" })),
    proposals: [],
  };
}

function replyTurn(content: string, proposals: ChatProposal[] = []) {
  return { role: "assistant" as const, content, attachments: [], proposals };
}

const PROPOSAL: ChatProposal = {
  path: "Launch.md",
  summary: "Fold the two intros together",
  before_hash: "before",
  new_content: "one intro, folded",
  hunks: [],
  status: "pending",
};

/** The store settles a stream by reading the file back, so every test states
 * what the file holds afterwards. */
function fileAfter(id: string, turns: ChatConversation["turns"]) {
  mocks.chatOpen.mockResolvedValue(conversation(id, turns));
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.chatList.mockResolvedValue([]);
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatSend.mockResolvedValue({ conversation_id: "c1", attached: [] });
  mocks.chatCancel.mockResolvedValue(undefined);
  mocks.chatDiscardProposal.mockResolvedValue(undefined);
  mocks.chatAttachedSizes.mockResolvedValue([]);
  chatStore.reset();
  for (const note of chatStore.attachments()) chatStore.detach(note.path);
});

describe("opening the pane", () => {
  it("shows the conversation last written to", async () => {
    mocks.chatList.mockResolvedValue([
      summary("newer", "Yesterday", "2026-09-14T10:00:00+00:00"),
      summary("older", "Last week", "2026-09-08T10:00:00+00:00"),
    ]);
    mocks.chatOpen.mockResolvedValue(
      conversation("newer", [userTurn("what does it argue"), replyTurn("it argues this")]),
    );

    await chatStore.openPane();

    expect(mocks.chatOpen).toHaveBeenCalledWith("newer");
    expect(chatStore.current()?.id).toBe("newer");
    expect(chatStore.messages().map((turn) => turn.content)).toEqual([
      "what does it argue",
      "it argues this",
    ]);
  });

  it("renders every stored reply", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(
      conversation("c1", [userTurn("ask"), replyTurn("# answer")]),
    );

    await chatStore.openPane();

    expect(mocks.chatRenderReply).toHaveBeenCalledWith("# answer");
    expect(chatStore.messages()[1].html).toBe("<p># answer</p>");
  });

  it("opens nothing when no conversation has been written", async () => {
    await chatStore.openPane();
    expect(mocks.chatOpen).not.toHaveBeenCalled();
    expect(chatStore.current()).toBeNull();
  });
});

describe("sending", () => {
  it("creates the conversation on the first send", async () => {
    mocks.chatNew.mockResolvedValue(conversation("c1"));
    chatStore.setDraft("what does it argue");

    await chatStore.send();

    expect(mocks.chatNew).toHaveBeenCalledTimes(1);
    expect(mocks.chatSend).toHaveBeenCalledWith("c1", "what does it argue", [], undefined);
    expect(chatStore.status()).toBe("thinking");
    expect(chatStore.draft()).toBe("");
  });

  it("carries the attached paths", async () => {
    mocks.chatNew.mockResolvedValue(conversation("c1"));
    chatStore.attach({ path: "/notes/Launch.md", name: "Launch.md", bytes: 12 });
    chatStore.setDraft("tighten the opening");

    await chatStore.send();

    expect(mocks.chatSend).toHaveBeenCalledWith(
      "c1",
      "tighten the opening",
      ["/notes/Launch.md"],
      undefined,
    );
  });

  it("leaves the words in the composer when the send is refused", async () => {
    mocks.chatNew.mockResolvedValue(conversation("c1"));
    mocks.chatSend.mockRejectedValue("This chat is full. Start a new chat to continue.");
    chatStore.setDraft("one more question");

    await chatStore.send();

    expect(chatStore.draft()).toBe("one more question");
    expect(chatStore.status()).toBe("error");
    expect(chatStore.errorMessage()).toBe("This chat is full. Start a new chat to continue.");
    expect(chatStore.messages()).toHaveLength(0);
  });
});

describe("the stream", () => {
  async function startSend() {
    mocks.chatNew.mockResolvedValue(conversation("c1"));
    chatStore.setDraft("what does it argue");
    await chatStore.send();
  }

  it("waits, then streams, then settles on what the file holds", async () => {
    await startSend();
    expect(chatStore.status()).toBe("thinking");

    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text: "it " });
    expect(chatStore.status()).toBe("streaming");
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text: "argues this" });
    expect(chatStore.messages()[1].content).toBe("it argues this");

    fileAfter("c1", [userTurn("what does it argue"), replyTurn("it argues this")]);
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "done", proposals: [] });
    expect(chatStore.status()).toBe("done");
    await flush();

    expect(chatStore.current()?.turns).toHaveLength(2);
    expect(chatStore.messages()[1].html).toBe("<p>it argues this</p>");
  });

  it("keeps a tail that arrives with nothing shown before it", async () => {
    await startSend();
    expect(chatStore.status()).toBe("thinking");

    // A reply the filter withheld whole releases its tail when the stream
    // ends, one frame before the ending, so the first chunk can be the last.
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text: "```" });
    fileAfter("c1", [userTurn("what does it argue"), replyTurn("```")]);
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "done", proposals: [] });
    await flush();

    expect(chatStore.messages()[1].content).toBe("```");
    expect(chatStore.current()?.turns).toHaveLength(2);
  });

  it("renders a live reply no more than once per throttle window", async () => {
    vi.useFakeTimers();
    try {
      await startSend();
      for (const text of ["a", "b", "c"]) {
        chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text });
      }
      expect(mocks.chatRenderReply).not.toHaveBeenCalled();
      vi.advanceTimersByTime(RENDER_THROTTLE_MS);
      expect(mocks.chatRenderReply).toHaveBeenCalledTimes(1);
      expect(mocks.chatRenderReply).toHaveBeenCalledWith("abc");
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the text a stopped reply had already shown", async () => {
    await startSend();
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text: "half an ans" });
    fileAfter("c1", [userTurn("what does it argue"), replyTurn("half an ans")]);

    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "stopped" });
    await flush();

    expect(chatStore.status()).toBe("stopped");
    expect(chatStore.messages()[1].content).toBe("half an ans");
  });

  it("leaves a pane with nothing in flight as it is", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(
      conversation("c1", [userTurn("what does it argue"), replyTurn("it argues this")]),
    );
    await chatStore.openPane();
    expect(chatStore.status()).toBe("idle");

    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "stopped" });

    expect(chatStore.status()).toBe("idle");
  });

  it("ignores a frame from another conversation", async () => {
    await startSend();
    chatStore.handleStreamEvent({ conversation_id: "other", kind: "chunk", text: "not mine" });
    expect(chatStore.messages()[1].content).toBe("");
  });

  it("keeps only the newest render of a turn", async () => {
    await startSend();
    let release = (_html: string) => {};
    mocks.chatRenderReply
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            release = resolve;
          }),
      )
      .mockResolvedValue("<p>the whole reply</p>");
    fileAfter("c1", [userTurn("what does it argue"), replyTurn("the whole reply")]);
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text: "the whole" });

    await new Promise((resolve) => setTimeout(resolve, RENDER_THROTTLE_MS + 5));
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "done", proposals: [] });
    await flush();
    release("<p>the whole</p>");
    await flush();

    expect(chatStore.messages()[1].html).toBe("<p>the whole reply</p>");
  });
});

describe("recovering from an error", () => {
  it("sends the same words and the same notes again, in place of the turn that failed", async () => {
    mocks.chatNew.mockResolvedValue(conversation("c1"));
    chatStore.attach({ path: "/notes/Launch.md", name: "Launch.md", bytes: 12 });
    chatStore.setDraft("what does it argue");
    await chatStore.send();

    fileAfter("c1", [userTurn("what does it argue", ["Launch.md"])]);
    chatStore.handleStreamEvent({
      conversation_id: "c1",
      kind: "error",
      text: "The model did not answer.",
    });
    await flush();
    expect(chatStore.status()).toBe("error");
    expect(chatStore.errorMessage()).toBe("The model did not answer.");

    mocks.chatSend.mockClear();
    await chatStore.retry();

    expect(mocks.chatSend).toHaveBeenCalledWith(
      "c1",
      "what does it argue",
      ["/notes/Launch.md"],
      0,
    );
    expect(chatStore.status()).toBe("thinking");
  });
});

describe("editing a sent turn", () => {
  // The file stores an attachment by its folder-relative key, which is the
  // shape `chat_send` takes back. The chip carries the key unchanged and shows
  // the note's own name.
  it("puts it back in the composer with the notes it carried", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(
      conversation("c1", [
        userTurn("first question", ["Ideas/Launch.md"]),
        replyTurn("first answer"),
        userTurn("second question"),
        replyTurn("second answer"),
      ]),
    );
    await chatStore.openPane();

    chatStore.beginEdit(0);

    expect(chatStore.draft()).toBe("first question");
    expect(chatStore.editing()).toBe(0);
    expect(chatStore.attachments()).toEqual([
      { path: "Ideas/Launch.md", name: "Launch.md", bytes: 10 },
    ]);
    expect(chatStore.isAttached("Ideas/Launch.md")).toBe(true);

    // The size read joins on the path it was asked about, so the key works
    // there too.
    mocks.chatAttachedSizes.mockResolvedValue([
      { path: "Ideas/Launch.md", key: "Ideas/Launch.md", bytes: 42 },
    ]);
    expect(await chatStore.attachedOnDisk()).toEqual([
      { path: "Ideas/Launch.md", name: "Launch.md", bytes: 42 },
    ]);

    chatStore.setDraft("a better first question");
    await chatStore.send();
    expect(mocks.chatSend).toHaveBeenCalledWith(
      "c1",
      "a better first question",
      ["Ideas/Launch.md"],
      0,
    );
  });

  it("replaces that turn and everything after it", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(
      conversation("c1", [
        userTurn("first question"),
        replyTurn("first answer"),
        userTurn("second question"),
        replyTurn("second answer"),
      ]),
    );
    await chatStore.openPane();

    chatStore.beginEdit(2);
    chatStore.setDraft("a better second question");
    await chatStore.send();

    expect(mocks.chatSend).toHaveBeenCalledWith("c1", "a better second question", [], 2);
    expect(chatStore.messages().map((turn) => turn.content)).toEqual([
      "first question",
      "first answer",
      "a better second question",
      "",
    ]);
    expect(chatStore.editing()).toBeNull();
  });
});

describe("the turns on screen", () => {
  it("keeps the object of a turn a frame did not change", async () => {
    mocks.chatNew.mockResolvedValue(conversation("c1"));
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text: "it " });
    const asked = chatStore.messages()[0];

    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text: "argues this" });

    expect(chatStore.messages()[0]).toBe(asked);

    // The reload hands back equal turns in new arrays, which is the case a
    // reference check misses.
    fileAfter("c1", [userTurn("what does it argue"), replyTurn("it argues this")]);
    chatStore.handleStreamEvent({ conversation_id: "c1", kind: "done", proposals: [] });
    await flush();

    expect(chatStore.messages()[0]).toBe(asked);
  });
});

describe("a send that was refused", () => {
  it("keeps the turns the file still holds, and still knows it was an edit", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(
      conversation("c1", [
        userTurn("first question"),
        replyTurn("first answer"),
        userTurn("second question"),
        replyTurn("second answer"),
      ]),
    );
    await chatStore.openPane();

    chatStore.beginEdit(2);
    chatStore.setDraft("a better second question");
    mocks.chatSend.mockRejectedValue("The model did not answer.");
    await chatStore.send();

    expect(chatStore.messages().map((turn) => turn.content)).toEqual([
      "first question",
      "first answer",
      "second question",
      "second answer",
    ]);
    expect(chatStore.editing()).toBe(2);
    expect(chatStore.draft()).toBe("a better second question");

    mocks.chatSend.mockReset().mockResolvedValue({ conversation_id: "c1", attached: [] });
    await chatStore.send();

    expect(mocks.chatSend).toHaveBeenCalledWith("c1", "a better second question", [], 2);
  });

  it("keeps a refused retry's words in the pane and the composer as it found it", async () => {
    mocks.chatNew.mockResolvedValue(conversation("c1"));
    chatStore.setDraft("what does it argue");
    await chatStore.send();

    fileAfter("c1", [userTurn("what does it argue")]);
    chatStore.handleStreamEvent({
      conversation_id: "c1",
      kind: "error",
      text: "The model did not answer.",
    });
    await flush();

    chatStore.setDraft("a half-typed next question");
    mocks.chatSend.mockRejectedValue("The model did not answer.");
    await chatStore.retry();

    expect(chatStore.messages().map((turn) => turn.content)).toEqual(["what does it argue"]);
    expect(chatStore.draft()).toBe("a half-typed next question");
  });
});

describe("a proposal", () => {
  async function openWithProposal() {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(
      conversation("c1", [userTurn("tighten it"), replyTurn("here it is", [PROPOSAL])]),
    );
    await chatStore.openPane();
  }

  it("is applied at the turn the file holds it at", async () => {
    await openWithProposal();
    mocks.chatApplyProposal.mockResolvedValue({ path: "Launch.md", written: true });

    await chatStore.apply(1, PROPOSAL);

    expect(mocks.chatApplyProposal).toHaveBeenCalledWith(
      "c1",
      1,
      "Launch.md",
      "one intro, folded",
      "before",
    );
    expect(chatStore.messages()[1].proposals[0].status).toBe("applied");
  });

  it("shows why a refused apply was refused", async () => {
    await openWithProposal();
    mocks.chatApplyProposal.mockRejectedValue("The note changed since this was offered.");

    await chatStore.apply(1, PROPOSAL);

    expect(chatStore.messages()[1].proposals[0].status).toBe("refused");
    expect(chatStore.refusalFor(1, "Launch.md")).toBe("The note changed since this was offered.");
  });

  it("is discarded at the same turn", async () => {
    await openWithProposal();

    await chatStore.discard(1, PROPOSAL);

    expect(mocks.chatDiscardProposal).toHaveBeenCalledWith("c1", 1, "Launch.md");
    expect(chatStore.messages()[1].proposals[0].status).toBe("discarded");
  });
});

describe("the conversation list", () => {
  it("renames and reads the list again", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(conversation("c1"));
    await chatStore.openPane();
    mocks.chatRename.mockResolvedValue({ ...conversation("c1"), title: "Launch copy" });

    await chatStore.rename("c1", "Launch copy");

    expect(mocks.chatRename).toHaveBeenCalledWith("c1", "Launch copy");
    expect(chatStore.current()?.title).toBe("Launch copy");
  });

  it("deleting the open conversation clears the pane", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(conversation("c1", [userTurn("ask")]));
    await chatStore.openPane();
    mocks.chatDelete.mockResolvedValue(undefined);
    mocks.chatList.mockResolvedValue([]);

    await chatStore.remove("c1");

    expect(chatStore.current()).toBeNull();
    expect(chatStore.messages()).toHaveLength(0);
  });

  it("a new chat writes no file until something is sent", async () => {
    mocks.chatList.mockResolvedValue([summary("c1", "A chat", "2026-09-14T10:00:00+00:00")]);
    mocks.chatOpen.mockResolvedValue(conversation("c1", [userTurn("ask")]));
    await chatStore.openPane();

    chatStore.newChat();

    expect(mocks.chatNew).not.toHaveBeenCalled();
    expect(chatStore.current()).toBeNull();
    expect(chatStore.messages()).toHaveLength(0);
  });
});
