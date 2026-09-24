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
  rename: vi.fn(),
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
      rename: mocks.rename,
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
  mocks.rename.mockReset().mockResolvedValue(undefined);
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

  // Removing the focused field makes the browser blur it, and the field
  // commits on blur, so Enter and Escape are each followed by one. jsdom does
  // not blur a removed node, so the tests fire it.
  it("renames once on Enter, and not again on the blur that follows", () => {
    const { getByRole } = render(() => <ConversationList onPick={() => undefined} />);
    fireEvent.click(getByRole("button", { name: "Rename Launch" }));
    const field = getByRole("textbox", { name: "Rename Launch" }) as HTMLInputElement;
    field.value = "Launch plan";
    fireEvent.keyDown(field, { key: "Enter" });
    fireEvent.blur(field);
    expect(mocks.rename).toHaveBeenCalledTimes(1);
    expect(mocks.rename).toHaveBeenCalledWith("c-1", "Launch plan");
  });

  it("renames nothing on Escape, whatever the blur that follows carries", () => {
    const { getByRole } = render(() => <ConversationList onPick={() => undefined} />);
    fireEvent.click(getByRole("button", { name: "Rename Launch" }));
    const field = getByRole("textbox", { name: "Rename Launch" }) as HTMLInputElement;
    field.value = "Launch plan";
    fireEvent.keyDown(field, { key: "Escape" });
    fireEvent.blur(field);
    expect(mocks.rename).not.toHaveBeenCalled();
  });
});
