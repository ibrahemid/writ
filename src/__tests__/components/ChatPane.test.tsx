import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { configStore } from "../../stores/global/config";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import type { BufferDocument } from "../../types/buffer";
import type { WritConfig } from "../../types/config";

// The chat column: what it says the model can read, what a reply may offer,
// and what happens to the note when somebody answers the offer. Nothing here
// writes a file — applying is one call to one service, and a note that changed
// since the offer was made comes back refused.

const mocks = vi.hoisted(() => ({
  chatSend: vi.fn(),
  chatCancel: vi.fn().mockResolvedValue(undefined),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn().mockResolvedValue(undefined),
  chatState: vi.fn(),
  config: vi.fn(),
  activeTabs: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  chatSend: mocks.chatSend,
  chatCancel: mocks.chatCancel,
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

vi.spyOn(configStore, "config").mockImplementation(() => mocks.config());
vi.spyOn(bufferRegistry, "activeTabs").mockImplementation(() => mocks.activeTabs());

import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import ChatPane from "../../components/Chat/ChatPane";
import { chatStore } from "../../stores/global/chat";

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
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: {
    enabled: false,
    preset: "ollama",
    base_url: "http://localhost:11434/v1",
    model: "",
    consented_hosts: [],
    chat: {
      enabled: true,
      provider: "openai_compatible",
      base_url: "http://localhost:11434/v1",
      model: "llama3",
    },
  },
  mcp: { enabled: false, approved_clients: [] },
  spelling: { enabled: false, dialect: "american", ignored_words: [] },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "split",
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
  chatStore.handleStreamEvent({ conversation_id: id, kind: "chunk", text });
  chatStore.handleStreamEvent({
    conversation_id: id,
    kind: "done",
    proposals: [PROPOSAL],
  });
  return id;
}

describe("the chat column", () => {
  beforeEach(() => {
    mocks.config.mockReturnValue(config(true));
    mocks.activeTabs.mockReturnValue([note("L1", LAUNCH), note("O1", OTHER)]);
    mocks.chatSend.mockReset().mockImplementation((conversationId: string) =>
      Promise.resolve({
        conversation_id: conversationId,
        attached: [{ path: "Launch.md", text: "the first text\n", before_hash: "abc" }],
      }),
    );
    mocks.chatApplyProposal
      .mockReset()
      .mockResolvedValue({ path: "Launch.md", hash: "def", bytes: 16 });
    mocks.chatDiscardProposal.mockReset().mockResolvedValue(undefined);
    mocks.chatCancel.mockReset().mockResolvedValue(undefined);
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
    await waitFor(() =>
      expect(container.querySelectorAll(".chat-attached-row")).toHaveLength(1),
    );
    expect(container.querySelector(".chat-attached-name")?.textContent).toBe("Launch.md");

    chatStore.setDraft("what does it argue");
    await chatStore.send();

    expect(mocks.chatSend).toHaveBeenCalledTimes(1);
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([LAUNCH]);
  });

  it("sends the notes a person attached, and stops sending one they removed", async () => {
    const { container, getByText } = open();
    await waitFor(() =>
      expect(container.querySelectorAll(".chat-attached-row")).toHaveLength(1),
    );

    fireEvent.click(getByText("Attach a note"));
    fireEvent.click(await waitFor(() => getByText("Other.md")));
    await waitFor(() =>
      expect(container.querySelectorAll(".chat-attached-row")).toHaveLength(2),
    );

    chatStore.detach(LAUNCH);
    await waitFor(() =>
      expect(container.querySelectorAll(".chat-attached-row")).toHaveLength(1),
    );

    chatStore.setDraft("and this one");
    await chatStore.send();
    expect(mocks.chatSend.mock.calls[0][2]).toEqual([OTHER]);
  });

  it("renders a proposal beside the text the model was given", async () => {
    const { container } = open();
    await exchange();

    await waitFor(() => expect(container.querySelector(".chat-proposal")).not.toBeNull());
    const panes = container.querySelectorAll(".chat-proposal-body");
    expect(panes).toHaveLength(2);
    expect(panes[0].textContent).toContain("the first text");
    expect(panes[1].textContent).toContain("the second text");
    expect(container.querySelector(".chat-proposal-summary")?.textContent).toBe(
      "Fold the intros",
    );
  });

  it("applies through one call and says so", async () => {
    const { container, getByText } = open();
    await exchange();
    await waitFor(() => expect(container.querySelector(".chat-proposal")).not.toBeNull());

    fireEvent.click(getByText("Apply"));

    await waitFor(() => expect(mocks.chatApplyProposal).toHaveBeenCalledTimes(1));
    expect(mocks.chatApplyProposal).toHaveBeenCalledWith(
      "Launch.md",
      "the second text\n",
      "abc",
    );
    await waitFor(() =>
      expect(container.querySelector(".chat-proposal-verdict")?.textContent).toBe("Applied."),
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

    await waitFor(() => expect(mocks.chatDiscardProposal).toHaveBeenCalledWith("Launch.md"));
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
    chatStore.handleStreamEvent({ conversation_id: id, kind: "chunk", text: "half a th" });
    await waitFor(() => expect(container.textContent).toContain("half a th"));

    fireEvent.click(getByText("Stop"));

    expect(mocks.chatCancel).toHaveBeenCalledWith(id);
    expect(container.textContent).toContain("half a th");
    expect(container.querySelector(".chat-proposal")).toBeNull();

    // A frame that arrives after the stop changes nothing on screen.
    chatStore.handleStreamEvent({ conversation_id: id, kind: "chunk", text: "ought" });
    expect(container.textContent).not.toContain("ought");
  });
});
