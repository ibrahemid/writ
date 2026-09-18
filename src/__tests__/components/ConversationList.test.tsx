import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

// A reply keeps arriving into the chat it was sent from, so the list is where
// a person sees one running in a chat they are not reading, and stops it.

const mocks = vi.hoisted(() => ({
  conversations: vi.fn(),
  current: vi.fn(),
  isLive: vi.fn(),
  stop: vi.fn(),
  open: vi.fn(),
}));

vi.mock("../../stores/global/chat", async () => {
  const actual = await vi.importActual<typeof import("../../stores/global/chat")>(
    "../../stores/global/chat",
  );
  return {
    ...actual,
    chatStore: {
      conversations: mocks.conversations,
      current: mocks.current,
      isLive: mocks.isLive,
      stop: mocks.stop,
      open: mocks.open,
      rename: vi.fn(),
      remove: vi.fn(),
    },
  };
});

import ConversationList from "../../components/Chat/ConversationList";

function row(id: string, title: string) {
  return {
    id,
    title,
    created_at: "2026-09-17T09:00:00Z",
    updated_at: "2026-09-17T09:00:00Z",
    turns: 2,
  };
}

beforeEach(() => {
  mocks.conversations.mockReset().mockReturnValue([row("c-1", "Launch"), row("c-2", "Notes")]);
  mocks.current.mockReset().mockReturnValue({ id: "c-2" });
  mocks.isLive.mockReset().mockImplementation((id: string) => id === "c-1");
  mocks.stop.mockReset();
  mocks.open.mockReset();
});

afterEach(() => {
  cleanup();
});

describe("the chat list", () => {
  it("marks a chat that is still answering and stops it from its row", () => {
    const { container, getByRole } = render(() => <ConversationList onPick={() => undefined} />);

    const live = container.querySelectorAll(".chat-chats-live");
    expect(live.length).toBe(1);

    fireEvent.click(getByRole("button", { name: "Stop Launch" }));
    expect(mocks.stop).toHaveBeenCalledWith("c-1");
  });
});
