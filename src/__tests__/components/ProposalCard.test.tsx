import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// The card is the one place a reply reaches a note, and the write behind it is
// a person's click. It runs here over the real store: what the card says about
// a write is only true if the store's own in-flight state is what the buttons
// read.

const mocks = vi.hoisted(() => ({
  chatOpen: vi.fn(),
  chatRenderReply: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
  chatAttachedSizes: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: vi.fn().mockResolvedValue([]),
  chatOpen: mocks.chatOpen,
  chatNew: vi.fn(),
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: mocks.chatRenderReply,
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatApplyProposal: mocks.chatApplyProposal,
  chatDiscardProposal: mocks.chatDiscardProposal,
}));

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
}));

import ProposalCard from "../../components/Chat/ProposalCard";
import { chatStore } from "../../stores/global/chat";

const PROPOSAL = {
  path: "Launch.md",
  before_hash: "abc",
  new_content: "Newer text\n",
  summary: "Tighten the opening",
  hunks: [{ before_start: 1, after_start: 1, lines: [{ kind: "added" as const, text: "Newer" }] }],
};

const CONVERSATION = {
  id: "c1",
  title: "A chat",
  created_at: "",
  updated_at: "",
  provider: "ollama",
  model: "a-model",
  turns: [
    { role: "user" as const, content: "tighten it", attachments: [], proposals: [] },
    { role: "assistant" as const, content: "here", attachments: [], proposals: [PROPOSAL] },
  ],
};

/** The card over the conversation the store actually holds, so the proposal it
 * renders is the one the store rewrites as the write lands. */
async function openCard(path?: string) {
  const held = JSON.parse(JSON.stringify(CONVERSATION)) as typeof CONVERSATION;
  if (path !== undefined) held.turns[1].proposals[0].path = path;
  mocks.chatOpen.mockResolvedValue(held);
  await chatStore.open("c1");
  const view = render(() => (
    <ProposalCard turn={1} proposal={chatStore.messages()[1].proposals[0]} />
  ));
  const apply = () => view.getByRole("button", { name: "Apply" }) as HTMLButtonElement;
  const discard = () => view.getByRole("button", { name: "Discard" }) as HTMLButtonElement;
  return { ...view, apply, discard };
}

beforeEach(() => {
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatAttachedSizes.mockResolvedValue([]);
  mocks.chatApplyProposal.mockResolvedValue({
    path: "Launch.md",
    hash: "def",
    bytes: 40,
    changed: true,
  });
  mocks.chatDiscardProposal.mockResolvedValue(undefined);
  chatStore.reset();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("a proposal card", () => {
  it("says the write is running, and takes one press while it is", async () => {
    let land: (outcome: unknown) => void = () => undefined;
    mocks.chatApplyProposal.mockReturnValue(
      new Promise((resolve) => {
        land = resolve;
      }),
    );
    const { apply, container } = await openCard();

    fireEvent.click(apply());
    await waitFor(() => expect(apply().disabled).toBe(true));
    expect(apply().getAttribute("aria-busy")).toBe("true");

    fireEvent.click(apply());
    expect(mocks.chatApplyProposal).toHaveBeenCalledTimes(1);

    land({ path: "Launch.md", hash: "def", bytes: 40, changed: true });
    await waitFor(() => expect(container.textContent).toContain("40 bytes"));
  });

  it("a refused apply can be retried and reaches IPC again", async () => {
    mocks.chatApplyProposal.mockRejectedValueOnce("Launch.md changed since the offer was made.");
    const { apply, container } = await openCard();

    fireEvent.click(apply());
    await waitFor(() =>
      expect(container.textContent).toContain("Launch.md changed since the offer was made."),
    );
    // The offer is still an offer: the note can be put back as it was and the
    // write tried again.
    expect(apply().disabled).toBe(false);
    expect(container.querySelector(".chat-diff")).toBeTruthy();

    fireEvent.click(apply());
    await waitFor(() => expect(mocks.chatApplyProposal).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(container.textContent).toContain("40 bytes"));
    expect(container.textContent).not.toContain("changed since the offer was made");
  });

  it("discard stays available after a refusal", async () => {
    mocks.chatApplyProposal.mockRejectedValue("No.");
    const { apply, discard, container } = await openCard();

    fireEvent.click(apply());
    await waitFor(() => expect(container.textContent).toContain("No."));

    expect(discard().disabled).toBe(false);
    fireEvent.click(discard());
    await waitFor(() =>
      expect(mocks.chatDiscardProposal).toHaveBeenCalledWith("c1", 1, "Launch.md"),
    );
  });

  it("says nothing changed when the note already held the text", async () => {
    mocks.chatApplyProposal.mockResolvedValue({
      path: "Launch.md",
      hash: "def",
      bytes: 40,
      changed: false,
    });
    const { apply, container } = await openCard();

    fireEvent.click(apply());

    await waitFor(() => expect(container.textContent).toContain("The note already held this text."));
    expect(mocks.chatAttachedSizes).not.toHaveBeenCalled();
  });

  it("a note in the folder gets no tip repeating the line it sits on", async () => {
    const { container } = await openCard("Ideas/Launch.md");

    const shown = container.querySelector(".chat-proposal-path") as HTMLElement;
    expect(shown.textContent).toBe("Ideas/Launch.md");
    expect(container.querySelector(".writ-tooltip-anchor")).toBeNull();
  });

  it("an outside proposal reads as its file name", async () => {
    const path = "/elsewhere/repo/README.md";
    const { container, apply } = await openCard(path);

    const shown = container.querySelector(".chat-proposal-path") as HTMLElement;
    expect(shown.textContent).toBe("README.md");
    expect(container.querySelector(".chat-proposal")?.getAttribute("aria-label")).toBe(
      "Change to README.md",
    );

    // Two files of one name in two repositories are told apart by the path,
    // which is what the tip under the pointer carries.
    vi.useFakeTimers();
    fireEvent.pointerEnter(container.querySelector(".writ-tooltip-anchor") as Element);
    vi.advanceTimersByTime(500);
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe(path);
    vi.useRealTimers();

    fireEvent.click(apply());

    await waitFor(() =>
      expect(mocks.chatApplyProposal).toHaveBeenCalledWith(
        "c1",
        1,
        path,
        PROPOSAL.new_content,
        PROPOSAL.before_hash,
      ),
    );
  });

  it("waits for the store before it says the offer is gone", async () => {
    let settle: () => void = () => undefined;
    mocks.chatDiscardProposal.mockReturnValue(
      new Promise<void>((resolve) => {
        settle = resolve;
      }),
    );
    const { discard, container } = await openCard();

    fireEvent.click(discard());
    await waitFor(() => expect(discard().disabled).toBe(true));
    expect(container.textContent).not.toContain("Discarded.");

    settle();
    await waitFor(() => expect(container.textContent).toContain("Discarded."));
  });
});
