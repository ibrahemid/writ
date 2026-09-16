import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// The card is the one place a reply reaches a note, and the write behind it is
// a person's click. What it says afterwards has to be true of the file: a
// write that moved nothing says so, and a refused one keeps the offer.

const mocks = vi.hoisted(() => ({
  apply: vi.fn(),
  discard: vi.fn(),
  refusal: vi.fn(),
  applying: vi.fn(),
}));

vi.mock("../../stores/global/chat", async () => {
  const actual = await vi.importActual<typeof import("../../stores/global/chat")>(
    "../../stores/global/chat",
  );
  return {
    ...actual,
    chatStore: {
      apply: mocks.apply,
      discard: mocks.discard,
      refusalFor: mocks.refusal,
      isApplying: mocks.applying,
    },
  };
});

vi.mock("../../commands/chat", () => ({
  byteLabel: (bytes: number) => `${bytes} bytes`,
}));

import ProposalCard from "../../components/Chat/ProposalCard";
import type { ChatProposal } from "../../stores/global/chat";

function proposal(over: Partial<ChatProposal> = {}): ChatProposal {
  return {
    path: "Launch.md",
    summary: "Tighten the opening",
    before_hash: "abc",
    new_content: "Newer text",
    hunks: [{ before_start: 1, after_start: 1, lines: [{ kind: "added", text: "Newer text" }] }],
    ...over,
  } as ChatProposal;
}

beforeEach(() => {
  mocks.apply.mockReset().mockResolvedValue({ path: "Launch.md", hash: "d", bytes: 40, changed: true });
  mocks.discard.mockReset().mockResolvedValue(undefined);
  mocks.refusal.mockReset().mockReturnValue(undefined);
  mocks.applying.mockReset().mockReturnValue(false);
});

afterEach(() => {
  cleanup();
});

describe("a proposal card", () => {
  it("takes one press while a write is running", () => {
    mocks.applying.mockReturnValue(true);
    const { getByRole } = render(() => <ProposalCard turn={1} proposal={proposal()} />);
    const apply = getByRole("button", { name: "Apply" }) as HTMLButtonElement;

    expect(apply.disabled).toBe(true);
    expect(apply.getAttribute("aria-busy")).toBe("true");

    fireEvent.click(apply);
    expect(mocks.apply).not.toHaveBeenCalled();
  });

  it("says how big the note is once it is written", async () => {
    const { getByRole, container } = render(() => <ProposalCard turn={1} proposal={proposal()} />);

    fireEvent.click(getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(container.textContent).toContain("40 bytes"));
    expect(mocks.apply).toHaveBeenCalledTimes(1);
  });

  it("says nothing changed when the note already held the text", async () => {
    mocks.apply.mockResolvedValue({ path: "Launch.md", hash: "d", bytes: 40, changed: false });
    const { getByRole, container } = render(() => <ProposalCard turn={1} proposal={proposal()} />);

    fireEvent.click(getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(container.textContent).toContain("The note already held this text."),
    );
  });

  it("keeps the offer when the write is refused", async () => {
    mocks.refusal.mockReturnValue("The note changed since this was offered.");
    const { container, getByRole } = render(() => (
      <ProposalCard turn={1} proposal={proposal({ status: "refused", stale: true })} />
    ));

    expect(container.textContent).toContain("The note changed since this was offered.");
    expect(container.querySelector(".chat-diff")).toBeTruthy();
    expect(getByRole("button", { name: "Apply" })).toBeTruthy();
  });

  it("waits for the store before it says the offer is gone", async () => {
    let settle: () => void = () => undefined;
    mocks.discard.mockReturnValue(
      new Promise<void>((resolve) => {
        settle = resolve;
      }),
    );
    const { getByRole } = render(() => <ProposalCard turn={1} proposal={proposal()} />);

    fireEvent.click(getByRole("button", { name: "Discard" }));
    await waitFor(() =>
      expect((getByRole("button", { name: "Discard" }) as HTMLButtonElement).disabled).toBe(true),
    );

    settle();
    await waitFor(() =>
      expect((getByRole("button", { name: "Discard" }) as HTMLButtonElement).disabled).toBe(false),
    );
  });
});
