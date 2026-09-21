import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { configStore } from "../../stores/global/config";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import type { BufferDocument } from "../../types/buffer";
import type { WritConfig } from "../../types/config";
import type { ChatConversation } from "../../services/tauri";

// The chat column: what it says the model can read, what a reply may offer,
// and what happens to the note when somebody answers the offer. Nothing here
// writes a file — applying is one call to one service, and a note that changed
// since the offer was made comes back refused.

const mocks = vi.hoisted(() => ({
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatRenderReply: vi.fn(),
  chatAttachedSizes: vi.fn(),
  noteNameCandidates: vi.fn(),
  chatNew: vi.fn(),
  chatSend: vi.fn(),
  chatStop: vi.fn().mockResolvedValue(undefined),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn().mockResolvedValue(undefined),
  chatState: vi.fn(),
  config: vi.fn(),
  activeTabs: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  chatList: mocks.chatList,
  chatOpen: mocks.chatOpen,
  chatRenderReply: mocks.chatRenderReply,
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  noteNameCandidates: mocks.noteNameCandidates,
  chatNew: mocks.chatNew,
  chatSend: mocks.chatSend,
  chatStop: mocks.chatStop,
  chatApplyProposal: mocks.chatApplyProposal,
  chatDiscardProposal: mocks.chatDiscardProposal,
  chatState: mocks.chatState,
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  searchBuffers: vi.fn().mockResolvedValue([]),
}));

// The pane's Send goes through the blocker pass, which opens dialogs and
// settings; here it is the send itself that is under test.
vi.mock("../../commands/chat", async () => {
  const { chatStore } = await import("../../stores/global/chat");
  return {
    byteLabel: (bytes: number) => `${bytes} bytes`,
    sendChatMessage: () => chatStore.send(),
  };
});

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
}));

vi.spyOn(configStore, "config").mockImplementation(() => mocks.config());
vi.spyOn(bufferRegistry, "activeTabs").mockImplementation(() => mocks.activeTabs());

import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import ChatPane from "../../components/Chat/ChatPane";
import { chatStore } from "../../stores/global/chat";

/** The id the last send on that conversation was minted with. Every frame of
 * an exchange carries it, and a frame that does not is a leftover. */
function rid(id: string): string {
  const calls = mocks.chatSend.mock.calls as unknown[][];
  for (let index = calls.length - 1; index >= 0; index -= 1) {
    if (calls[index][0] === id) return calls[index][4] as string;
  }
  return "no-such-request";
}


const LAUNCH = "/notes/Launch.md";
const OTHER = "/notes/Other.md";

function note(id: string, path: string): BufferDocument {
  return {
    id,
    title: path.split("/").pop() ?? path,
    filename: path.split("/").pop() ?? path,
    status: "active",
    language: null,
    source_path: path,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 120,
    line_ending: "lf",
  };
}

function config(chatEnabled: boolean): WritConfig {
  const base = JSON.parse(JSON.stringify(DEFAULTS)) as WritConfig;
  base.ai.chat.enabled = chatEnabled;
  return base;
}

// A whole config the pane can read, kept here rather than reached for through
// the store the test has already replaced.
const DEFAULTS: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: {
    toggle: "CmdOrCtrl+\\",
    default_visible: false,
    position: "left",
    open: false,
    width: 240,
    collapsed: [],
    hidden: [],
  },
  panel: { open: false, width: 240 },
  chat_panel: { open: false, width: 380 },
  first_run: { hint_dismissed: false },
  editor: {
    font_family: "monospace",
    font_size: 14,
    word_wrap: true,
    tab_size: 2,
    autosave_debounce_ms: 300,
    markdown_typography: true,
    markdown_editing: true,
    status_bar: false,
  },
  window: { width: 1100, height: 720, maximized: false },
  keybindings: {},
  history: { max_entries: 500 },
  storage: { path: "~/.writ" },
  theme: { preset: "warp-dark", overrides: {} },
  appearance: {
    polarity: "system",
    accent: "pine",
    prose_face: "system",
    interface_text_size: null,
  },
  commands: { usage: {} },
  files: { default_extension: "txt" },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: {
    provider: "ollama",
    base_url: "",
    model: "",
    consented_hosts: [],
    rewrite: { enabled: false },
    chat: { enabled: true, model: "llama3", model_provider: "" },
  },
  mcp: { enabled: false, approved_clients: [] },
  spelling: { enabled: false, dialect: "american", ignored_words: [] },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "inline",
    live_render_threshold_mb: 1,
    render_confirm_threshold_mb: 5,
    render_refuse_threshold_mb: 50,
    debounce_ms: 200,
    run_scripts: true,
  },
};

const PROPOSAL = {
  path: "Launch.md",
  before_hash: "abc",
  new_content: "the second text\n",
  summary: "Fold the intros",
  hunks: [
    {
      before_start: 1,
      after_start: 1,
      lines: [
        { kind: "removed" as const, text: "the first text" },
        { kind: "added" as const, text: "the second text" },
      ],
    },
  ],
};

function open() {
  const rendered = render(() => (
    <WindowProvider windowId={9101}>
      <ChatPane />
    </WindowProvider>
  ));
  const win = windowRegistry.getActive();
  win?.tabs.setActiveTabId("L1");
  win?.chatPanel.show();
  return rendered;
}

/** Runs one whole exchange: a message, a streamed reply, one proposal. */
async function exchange(text = "the reply text") {
  chatStore.setDraft("what does it argue");
  await chatStore.send();
  const calls = mocks.chatSend.mock.calls;
  const id = calls[calls.length - 1][0] as string;
  chatStore.handleStreamEvent({ conversation_id: id, request_id: rid(id), kind: "chunk", text });
  chatStore.handleStreamEvent({
    conversation_id: id, request_id: rid(id),
    kind: "done",
    proposals: [PROPOSAL],
  });
  return id;
}

describe("the chat column", () => {
  beforeEach(() => {
    mocks.config.mockReturnValue(config(true));
    mocks.activeTabs.mockReturnValue([note("L1", LAUNCH), note("O1", OTHER)]);
    mocks.chatList.mockReset().mockResolvedValue([]);
    mocks.chatOpen.mockReset().mockRejectedValue("no such chat");
    mocks.chatRenderReply.mockReset().mockImplementation(async (text: string) => `<p>${text}</p>`);
    mocks.chatAttachedSizes.mockReset().mockResolvedValue([{ path: OTHER, key: "Other.md", bytes: 40 }]);
    mocks.noteNameCandidates.mockReset().mockResolvedValue([{ path: OTHER, name: "Other" }]);
    mocks.chatNew.mockReset().mockImplementation(() =>
      Promise.resolve({
        id: `c-${mocks.chatNew.mock.calls.length}`,
        title: "New chat",
        created_at: "",
        updated_at: "",
        provider: "custom",
        model: "a-model",
        turns: [],
      }),
    );
    mocks.chatSend.mockReset().mockImplementation((conversationId: string) =>
      Promise.resolve({
        conversation_id: conversationId,
        attached: [{ path: "Launch.md", text: "the first text\n", before_hash: "abc" }],
      }),
    );
    mocks.chatApplyProposal
      .mockReset()
      .mockResolvedValue({ path: "Launch.md", hash: "def", bytes: 16, changed: true });
    mocks.chatDiscardProposal.mockReset().mockResolvedValue(undefined);
    mocks.chatStop.mockReset().mockResolvedValue(undefined);
    chatStore.reset();
    for (const held of chatStore.attachments()) chatStore.detach(held.path);
  });

  afterEach(() => {
    cleanup();
  });

  it("does not exist while the setting is off", () => {
    mocks.config.mockReturnValue(config(false));
    const { container } = render(() => (
      <WindowProvider windowId={9102}>
        <ChatPane />
      </WindowProvider>
    ));
    expect(container.querySelector(".chat-pane")).toBeNull();
  });

  it("lists the note in front and sends that one and no other", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    expect(container.querySelector(".chat-chip-name")?.textContent).toBe("Launch.md");

    chatStore.setDraft("what does it argue");
    await chatStore.send();

    expect(mocks.chatSend).toHaveBeenCalledTimes(1);
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([LAUNCH]);
  });

  it("attaches a note an @ named, and stops sending one they removed", async () => {
    const { container, getByText } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));

    const composer = container.querySelector(".chat-composer-input") as HTMLTextAreaElement;
    composer.value = "read @Oth";
    composer.setSelectionRange(9, 9);
    fireEvent.input(composer);

    const row = await waitFor(() => getByText("Other"));
    fireEvent.mouseDown(row);
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(2));
    expect(mocks.noteNameCandidates).toHaveBeenCalledWith("Oth", 8);
    expect(chatStore.draft()).toBe("read ");

    chatStore.detach(LAUNCH);
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));

    chatStore.setDraft("and this one");
    await chatStore.send();
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([OTHER]);
  });

  it("keeps a removed note out of what the next message carries", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));

    fireEvent.click(container.querySelector(".chat-chip-remove") as HTMLElement);

    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(0));
    chatStore.setDraft("without the note");
    await chatStore.send();
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([]);
  });

  it("the chip follows the tab in front", async () => {
    const { container } = open();
    await waitFor(() =>
      expect(container.querySelector(".chat-chip-name")?.textContent).toBe("Launch.md"),
    );

    windowRegistry.getActive()?.tabs.setActiveTabId("O1");

    await waitFor(() =>
      expect(container.querySelector(".chat-chip-name")?.textContent).toBe("Other.md"),
    );
    expect(container.querySelectorAll(".chat-chip")).toHaveLength(1);
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([OTHER]);
  });

  it("a chip sent away stays away while that tab is in front", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    fireEvent.click(container.querySelector(".chat-chip-remove") as HTMLElement);
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(0));

    windowRegistry.getActive()?.chatPanel.hide();
    windowRegistry.getActive()?.chatPanel.show();
    await Promise.resolve();

    expect(container.querySelectorAll(".chat-chip")).toHaveLength(0);
  });

  it("another tab in front brings the chip back", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    fireEvent.click(container.querySelector(".chat-chip-remove") as HTMLElement);
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(0));

    windowRegistry.getActive()?.tabs.setActiveTabId("O1");
    await waitFor(() =>
      expect(container.querySelector(".chat-chip-name")?.textContent).toBe("Other.md"),
    );

    windowRegistry.getActive()?.tabs.setActiveTabId("L1");
    await waitFor(() =>
      expect(container.querySelector(".chat-chip-name")?.textContent).toBe("Launch.md"),
    );
  });

  it("an unsaved tab in front leaves no chip and says why", async () => {
    mocks.activeTabs.mockReturnValue([
      note("L1", LAUNCH),
      { ...note("U1", LAUNCH), source_path: null },
    ]);
    const { container, getByText } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));

    windowRegistry.getActive()?.tabs.setActiveTabId("U1");

    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(0));
    expect(getByText("Save this file first")).toBeTruthy();
    expect((container.querySelector(".chat-chip-add") as HTMLButtonElement).disabled).toBe(true);
  });

  it("a tab switch while a reply arrives leaves the message in flight alone", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    const id = mocks.chatSend.mock.calls[0][0] as string;
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "chunk",
      text: "still going",
    });

    windowRegistry.getActive()?.tabs.setActiveTabId("O1");

    await waitFor(() =>
      expect(container.querySelector(".chat-chip-name")?.textContent).toBe("Other.md"),
    );
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([LAUNCH]);
    const sent = chatStore.messages().find((message) => message.role === "user");
    expect(sent?.attachments.map((held) => held.path)).toEqual([LAUNCH]);
  });

  it("starts a new chat with the note in front and nothing else", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    chatStore.attach({ path: OTHER, name: "Other.md", bytes: 40 });
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(2));

    fireEvent.click(container.querySelector('[aria-label="New chat"]') as HTMLElement);

    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    expect(container.querySelector(".chat-chip-name")?.textContent).toBe("Launch.md");
  });

  it("keeps an earlier turn, and what was copied from it, through a later reply", async () => {
    mocks.chatRenderReply.mockImplementation(
      async (text: string) => `<p>${text}</p><pre><code>echo hi</code></pre>`,
    );
    const { container } = open();
    await exchange("the first answer");
    await waitFor(() => expect(container.querySelector(".chat-code-copy")).not.toBeNull());

    const asked = container.querySelector(".chat-turn") as HTMLElement;
    const copy = container.querySelector(".chat-code-copy") as HTMLElement;
    fireEvent.click(copy);
    await waitFor(() => expect(copy.textContent).toBe("Copied"));

    chatStore.setDraft("and then");
    await chatStore.send();
    const calls = mocks.chatSend.mock.calls;
    const id = calls[calls.length - 1][0] as string;
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "chunk",
      text: "the second answer",
    });

    expect(container.querySelector(".chat-turn")).toBe(asked);
    expect(copy.isConnected).toBe(true);
    expect(copy.textContent).toBe("Copied");
  });

  it("does not chip a note twice for a second spelling of its path", async () => {
    mocks.chatAttachedSizes.mockResolvedValue([
      { path: OTHER, key: "Other.md", bytes: 40 },
      { path: "Other.md", key: "Other.md", bytes: 40 },
      { path: LAUNCH, key: "Launch.md", bytes: 120 },
    ]);
    const { container, getByText } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    // The shape a conversation file stores, which is what an edited turn
    // hands back to the composer.
    chatStore.attach({ path: "Other.md", name: "Other.md", bytes: 40 });
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(2));

    const composer = container.querySelector(".chat-composer-input") as HTMLTextAreaElement;
    composer.value = "read @Oth";
    composer.setSelectionRange(9, 9);
    fireEvent.input(composer);
    fireEvent.mouseDown(await waitFor(() => getByText("Other")));

    await waitFor(() => expect(chatStore.draft()).toBe("read "));
    expect(container.querySelectorAll(".chat-chip")).toHaveLength(2);
    expect(chatStore.attachments().map((note) => note.path)).toEqual([LAUNCH, "Other.md"]);
  });

  it("does not chip the note in front a second time for the file's spelling", async () => {
    mocks.chatAttachedSizes.mockResolvedValue([
      { path: LAUNCH, key: "Launch.md", bytes: 120 },
      { path: "Launch.md", key: "Launch.md", bytes: 120 },
    ]);
    // The shape a conversation file stores, which is what an edited turn hands
    // back to the composer before the pane is opened again.
    chatStore.attach({ path: "Launch.md", name: "Launch.md", bytes: 120 });

    const { container } = open();
    await waitFor(() => expect(mocks.chatAttachedSizes).toHaveBeenCalled());
    await waitFor(() => expect(chatStore.attachments()).toHaveLength(1));

    expect(container.querySelectorAll(".chat-chip")).toHaveLength(1);
    expect(chatStore.attachments().map((note) => note.path)).toEqual(["Launch.md"]);
  });

  it("shows a chat that would not open with nothing to retry", async () => {
    mocks.chatList.mockResolvedValue([
      { id: "c1", title: "A chat", created_at: "", updated_at: "", turns: 2 },
    ]);
    mocks.chatOpen.mockRejectedValue("This chat no longer exists.");

    const { container } = open();

    await waitFor(() => expect(container.querySelector(".chat-error")).not.toBeNull());
    expect(container.querySelector(".chat-error-text")?.textContent).toBe(
      "This chat no longer exists.",
    );
    expect(container.querySelector(".chat-error button")).toBeNull();
  });

  it("offers Retry for a send that did not leave", async () => {
    const { container } = open();
    mocks.chatSend.mockRejectedValue("The model did not answer.");
    chatStore.setDraft("what does it argue");
    await chatStore.send();

    await waitFor(() => expect(container.querySelector(".chat-error")).not.toBeNull());
    expect(container.querySelector(".chat-error button")?.textContent).toBe("Retry");
  });

  it("says what to do with an empty chat", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelector(".chat-empty")).not.toBeNull());
    expect(container.querySelector(".chat-transcript .chat-empty")?.textContent).toBe(
      "Ask about a file. Apply the change an answer offers, or discard it.",
    );
  });

  it("says what the field is for whether a note is attached or not", async () => {
    const { container } = open();
    await waitFor(() => expect(container.querySelectorAll(".chat-chip")).toHaveLength(1));
    const composer = container.querySelector(".chat-composer-input") as HTMLTextAreaElement;
    expect(composer.placeholder).toBe("Ask about the attached notes. @ attaches another.");

    fireEvent.click(container.querySelector(".chat-chip-remove") as HTMLElement);

    await waitFor(() => expect(composer.placeholder).toBe("@ attaches a note or a folder."));
  });

  it("shows a proposal as the lines it would change", async () => {
    const { container } = open();
    await exchange();

    await waitFor(() => expect(container.querySelector(".chat-proposal")).not.toBeNull());
    const rows = container.querySelectorAll(".chat-diff-row");
    expect(rows).toHaveLength(2);
    expect(rows[0].getAttribute("data-kind")).toBe("removed");
    expect(rows[0].textContent).toContain("the first text");
    expect(rows[1].getAttribute("data-kind")).toBe("added");
    expect(rows[1].textContent).toContain("the second text");
    expect(container.querySelector(".chat-proposal-summary")?.textContent).toBe(
      "Fold the intros",
    );
    // The whole text the note would hold is not on screen anywhere.
    expect(container.querySelector(".chat-proposal-body")).toBeNull();
  });

  it("applies through one call and says so", async () => {
    const { container, getByText } = open();
    await exchange();
    await waitFor(() => expect(container.querySelector(".chat-proposal")).not.toBeNull());

    fireEvent.click(getByText("Apply"));

    await waitFor(() => expect(mocks.chatApplyProposal).toHaveBeenCalledTimes(1));
    expect(mocks.chatApplyProposal).toHaveBeenCalledWith(
      expect.any(String),
      1,
      "Launch.md",
      "the second text\n",
      "abc",
    );
    await waitFor(() =>
      expect(container.querySelector(".chat-proposal-verdict")?.textContent).toBe(
        "Applied. The file is now 16 bytes.",
      ),
    );
  });

  it("shows the refusal when the note changed after the offer was made", async () => {
    // Word for word what `apply_proposal_inner` rejects with when the guard
    // refuses; the sentence is asserted against the command itself in
    // src-tauri/tests/chat_ipc_tests.rs.
    const REFUSAL =
      "Launch.md changed since the offer was made. " +
      "The proposed text is beside it in Launch (conflict 2026-09-10-120000).md.";
    mocks.chatApplyProposal.mockRejectedValue(REFUSAL);
    const { container, getByText } = open();
    await exchange();
    await waitFor(() => expect(container.querySelector(".chat-proposal")).not.toBeNull());

    fireEvent.click(getByText("Apply"));

    await waitFor(() =>
      expect(container.querySelector(".chat-proposal-verdict")?.textContent).toBe(REFUSAL),
    );
  });

  it("discarding records the offer and writes nothing", async () => {
    const { container, getByText } = open();
    await exchange();
    await waitFor(() => expect(container.querySelector(".chat-proposal")).not.toBeNull());

    fireEvent.click(getByText("Discard"));

    await waitFor(() =>
      expect(mocks.chatDiscardProposal).toHaveBeenCalledWith(expect.any(String), 1, "Launch.md"),
    );
    expect(mocks.chatApplyProposal).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(container.querySelector(".chat-proposal-verdict")?.textContent).toBe("Discarded."),
    );
  });

  it("stopping mid-reply keeps what arrived and offers nothing to apply", async () => {
    const { container, getByText } = open();
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    const id = mocks.chatSend.mock.calls[0][0] as string;
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "chunk",
      text: "half a th",
    });
    await waitFor(() => expect(container.textContent).toContain("half a th"));

    fireEvent.click(getByText("Stop"));
    chatStore.handleStreamEvent({ conversation_id: id, request_id: rid(id), kind: "stopped" });

    expect(mocks.chatStop).toHaveBeenCalledWith(id, expect.any(String));
    await waitFor(() => expect(container.textContent).toContain("half a th"));
    expect(container.querySelector(".chat-proposal")).toBeNull();

    // A frame that arrives after the stop changes nothing on screen.
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "chunk",
      text: "ought",
    });
    expect(container.textContent).not.toContain("ought");
  });
  it("says the reply stopped", async () => {
    const { container, getByText } = open();
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    const id = mocks.chatSend.mock.calls[0][0] as string;
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "chunk",
      text: "half a th",
    });
    await waitFor(() => expect(getByText("Stop")).toBeTruthy());

    chatStore.handleStreamEvent({ conversation_id: id, request_id: rid(id), kind: "stopped" });

    await waitFor(() => expect(container.textContent).toContain("The reply stopped."));
  });

  it("says nothing about an empty chat while one is being opened", async () => {
    let settle: (value: ChatConversation) => void = () => undefined;
    mocks.chatList.mockResolvedValue([
      { id: "c-9", title: "Launch", created_at: "", updated_at: "", turns: 2 },
    ]);
    mocks.chatOpen.mockReturnValue(
      new Promise<ChatConversation>((resolve) => {
        settle = resolve;
      }),
    );
    const { container } = open();

    await waitFor(() => expect(container.textContent).toContain("Opening this chat."));
    expect(container.textContent).not.toContain("Ask about a file.");

    settle({
      id: "c-9",
      title: "Launch",
      created_at: "",
      updated_at: "",
      provider: "ollama",
      model: "llama3",
      turns: [],
    });
    await waitFor(() => expect(container.textContent).toContain("Ask about a file."));
  });

  it("says what a reply could not offer, and which model wrote it", async () => {
    const { container } = open();
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    const id = mocks.chatSend.mock.calls[0][0] as string;
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "chunk",
      text: "here is what it argues",
    });
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "done",
      proposals: [],
      truncated: true,
      dropped: [
        { named: "Gone.md", reason: "unknown_note" },
        { named: "Launch.md", reason: "duplicate" },
      ],
      identity: { provider: "ollama", model: "llama3", host: "localhost:11434" },
    });

    await waitFor(() => expect(container.textContent).toContain("Reply was cut off."));
    expect(container.textContent).toContain(
      "An offer for Gone.md was not shown: that file is not attached.",
    );
    expect(container.textContent).toContain(
      "An offer for Launch.md was not shown: the same file was offered twice.",
    );
    expect(container.querySelector(".chat-identity")?.textContent).toBe("llama3 via ollama");
  });

  it("an error names the provider and the model that was refused", async () => {
    const { container, getByText } = open();
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    const id = mocks.chatSend.mock.calls[0][0] as string;
    chatStore.handleStreamEvent({
      conversation_id: id,
      request_id: rid(id),
      kind: "error",
      error: {
        kind: "model_unavailable",
        message: "llama3 is not available on Ollama.",
        provider: "ollama",
        model: "llama3",
        status: null,
      },
    });

    await waitFor(() =>
      expect(container.textContent).toContain("llama3 is not available on Ollama."),
    );
    expect(container.querySelector(".chat-error-identity")?.textContent).toBe("llama3 via ollama");
    expect(getByText("Change model")).toBeTruthy();
  });

  it("says what the connection still needs, above the composer", async () => {
    const { container } = open();

    await waitFor(() => expect(container.querySelector(".chat-readiness")).not.toBeNull());
    const banner = container.querySelector(".chat-readiness") as HTMLElement;
    expect(banner.getAttribute("data-state")).toBe("no_model");
    expect(banner.textContent).toContain("No model is set.");
    expect(banner.textContent).toContain("Open settings");
    // It sits between the conversation and the field it is about.
    expect(
      banner.compareDocumentPosition(container.querySelector(".chat-composer") as Node) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
