import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// A card that is already on screen when its note changes. Rust measures an
// offer against the note only when the conversation is read, so the card has
// to be read again on each change: an Apply the guard will refuse writes a
// dated copy beside the note every time it is pressed.

type Handler = (payload: unknown) => void;
type SavedListener = (bufferId: string, diskHash: string | null) => void;

const mocks = vi.hoisted(() => ({
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatRenderReply: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
  chatAttachedSizes: vi.fn(),
  chatUndoProposal: vi.fn(),
  openFile: vi.fn(),
}));

const bus = vi.hoisted(() => ({
  handlers: new Map<string, Handler[]>(),
  saved: [] as SavedListener[],
  tabs: [] as { id: string; source_path: string | null }[],
}));

vi.mock("../../services/tauri", () => ({
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: mocks.chatList,
  chatOpen: mocks.chatOpen,
  chatNew: vi.fn(),
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: mocks.chatRenderReply,
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatApplyProposal: mocks.chatApplyProposal,
  chatDiscardProposal: mocks.chatDiscardProposal,
  chatUndoProposal: mocks.chatUndoProposal,
}));

vi.mock("../../services/events", () => ({
  onEvent: (kind: string, handler: Handler) => {
    bus.handlers.set(kind, [...(bus.handlers.get(kind) ?? []), handler]);
    return Promise.resolve(() => undefined);
  },
}));

vi.mock("../../services/autosave", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/autosave")>()),
  onAutosaveSuccess: (listener: SavedListener) => {
    bus.saved.push(listener);
    return () => undefined;
  },
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => bus.tabs },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { openFile: mocks.openFile }, chatPanel: { show: vi.fn() } }),
  },
}));

vi.mock("../../stores/global/notes", () => ({
  notesStore: { root: () => "/notes", contains: () => true },
}));

import ProposalCard from "../../components/Chat/ProposalCard";
import { chatStore } from "../../stores/global/chat";
import type { ChatConversation, ChatProposal } from "../../services/tauri";

const PROPOSAL: ChatProposal = {
  path: "Launch.md",
  before_hash: "abc",
  new_content: "Newer text\n",
  summary: "Tighten the opening",
  hunks: [{ before_start: 1, after_start: 1, lines: [{ kind: "added", text: "Newer" }] }],
  status: "pending",
  stale: false,
};

/** The conversation as `chat_open` reads it, with the offer as Rust measures
 * it against the note at that moment. */
function fileWith(offer: Partial<ChatProposal> = {}): ChatConversation {
  return {
    id: "c1",
    title: "A chat",
    created_at: "",
    updated_at: "",
    provider: "ollama",
    model: "a-model",
    turns: [
      { role: "user", content: "tighten it", attachments: [], proposals: [] },
      {
        role: "assistant",
        content: "here",
        attachments: [],
        proposals: [{ ...PROPOSAL, ...offer }],
      },
    ],
  };
}

const MOVED: Partial<ChatProposal> = {
  stale: true,
  hunks: [{ before_start: 1, after_start: 1, lines: [{ kind: "removed", text: "Edited" }] }],
};

function emit(kind: string, payload: unknown) {
  for (const handler of bus.handlers.get(kind) ?? []) handler(payload);
}

function saveTab(id: string) {
  for (const listener of [...bus.saved]) listener(id, "new-hash");
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

async function openCard() {
  mocks.chatOpen.mockResolvedValue(fileWith());
  await chatStore.openPane();
  const view = render(() => (
    <ProposalCard turn={1} proposal={chatStore.messages()[1].proposals[0]} />
  ));
  const apply = () => view.getByRole("button", { name: "Apply" }) as HTMLButtonElement;
  return { ...view, apply };
}

beforeEach(() => {
  for (const mock of Object.values(mocks)) mock.mockReset();
  bus.tabs = [
    { id: "b1", source_path: "/notes/Launch.md" },
    { id: "b2", source_path: "/notes/Other.md" },
  ];
  mocks.chatList.mockResolvedValue([
    { id: "c1", title: "A chat", created_at: "", updated_at: "", turns: 2 },
  ]);
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatAttachedSizes.mockResolvedValue([]);
  mocks.chatDiscardProposal.mockResolvedValue(undefined);
  chatStore.reset();
  chatStore.setDraft("");
});

afterEach(() => {
  cleanup();
});

describe("a card whose note changes while it is on screen", () => {
  it("holds Apply back after Writ saves the note", async () => {
    const { apply, container, queryByRole } = await openCard();
    expect(apply().disabled).toBe(false);
    expect(queryByRole("button", { name: "Ask again" })).toBeNull();

    mocks.chatOpen.mockResolvedValue(fileWith(MOVED));
    saveTab("b1");

    await waitFor(() => expect(apply().disabled).toBe(true));
    expect(container.textContent).toContain("The file changed since this was offered.");
    expect(queryByRole("button", { name: "Ask again" })).not.toBeNull();
    fireEvent.click(apply());
    expect(mocks.chatApplyProposal).not.toHaveBeenCalled();
  });

  it("holds Apply back after another program writes the note, and offers it again once the note is back", async () => {
    const { apply, queryByRole } = await openCard();

    mocks.chatOpen.mockResolvedValue(fileWith(MOVED));
    emit("notes:changed", { path: "/notes/Launch.md", removed: false });
    await waitFor(() => expect(apply().disabled).toBe(true));

    mocks.chatOpen.mockResolvedValue(fileWith());
    emit("notes:changed", { path: "/notes/Launch.md", removed: false });
    await waitFor(() => expect(apply().disabled).toBe(false));
    expect(queryByRole("button", { name: "Ask again" })).toBeNull();
  });

  it("reads the note again after a refused Apply, so the next press is held back", async () => {
    const refusal =
      "Launch.md changed since the offer was made. The proposed text is beside it in Launch (conflict).md.";
    mocks.chatApplyProposal.mockRejectedValue(refusal);
    const { apply, container, queryByRole } = await openCard();

    mocks.chatOpen.mockResolvedValue(fileWith({ ...MOVED, status: "refused" }));
    fireEvent.click(apply());

    await waitFor(() => expect(queryByRole("button", { name: "Ask again" })).not.toBeNull());
    expect(apply().disabled).toBe(true);
    expect(container.textContent).toContain(refusal);
    fireEvent.click(apply());
    expect(mocks.chatApplyProposal).toHaveBeenCalledTimes(1);
  });

  it("reads nothing for a change to another note", async () => {
    const { apply } = await openCard();
    const reads = mocks.chatOpen.mock.calls.length;

    mocks.chatOpen.mockResolvedValue(fileWith(MOVED));
    saveTab("b2");
    emit("notes:changed", { path: "/notes/Other.md", removed: false });
    emit("buffer:external", {
      bufferId: "b2",
      path: "/notes/Other.md",
      change: "modified",
      newPath: null,
      diskHash: null,
    });
    await flush();

    expect(mocks.chatOpen.mock.calls.length).toBe(reads);
    expect(apply().disabled).toBe(false);
  });

  it("an applied card stays applied when the folder watcher reports its write", async () => {
    mocks.chatApplyProposal.mockResolvedValue({
      path: "Launch.md",
      hash: "def",
      bytes: 40,
      changed: true,
      undoable: true,
    });
    const { apply, container, getByRole } = await openCard();

    mocks.chatOpen.mockResolvedValue(fileWith({ status: "applied", hunks: [] }));
    fireEvent.click(apply());
    await waitFor(() => expect(container.textContent).toContain("Applied to Launch.md."));

    emit("notes:changed", { path: "/notes/Launch.md", removed: false });
    await flush();

    expect(container.textContent).toContain("Applied to Launch.md.");
    expect(container.textContent).not.toContain("The file changed since this was offered.");
    expect(getByRole("button", { name: "Undo" })).toBeTruthy();
  });
});
