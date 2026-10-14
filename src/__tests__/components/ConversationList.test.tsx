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
  listFailed: vi.fn(),
  refreshList: vi.fn(),
  requestConfirm: vi.fn(),
  remove: vi.fn(),
}));

vi.mock("../../stores/global/dialogs", () => ({ requestConfirm: mocks.requestConfirm }));

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
      remove: mocks.remove,
      listFailed: mocks.listFailed,
      refreshList: mocks.refreshList,
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
  mocks.listFailed.mockReset().mockReturnValue(false);
  mocks.refreshList.mockReset().mockResolvedValue(undefined);
  mocks.requestConfirm.mockReset().mockResolvedValue(true);
  mocks.remove.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe("the chat list", () => {
  it("says deleting a chat leaves the files alone", async () => {
    const { getByRole } = render(() => <ConversationList onPick={() => undefined} />);
    fireEvent.click(getByRole("button", { name: "Delete Notes" }));
    await vi.waitFor(() => expect(mocks.remove).toHaveBeenCalledWith("c-2"));
    const asked = mocks.requestConfirm.mock.calls[0][0];
    expect(asked.message).toBe("The conversation is deleted. Your files are not changed.");
    expect(asked.message).not.toContain("file goes");
  });

  it("names every icon in a row under the pointer", () => {
    const { container } = render(() => <ConversationList onPick={() => undefined} />);
    const live = container.querySelector(".chat-chats-row") as HTMLElement;
    const tipped = [...live.querySelectorAll(".writ-tooltip-anchor button")].map((button) =>
      button.getAttribute("aria-label"),
    );
    expect(tipped).toEqual(["Stop Launch", "Rename Launch", "Delete Launch"]);
  });

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

  it("renames nothing on the Enter that confirms an input method's conversion", () => {
    const { getByRole } = render(() => <ConversationList onPick={() => undefined} />);
    fireEvent.click(getByRole("button", { name: "Rename Launch" }));
    const field = getByRole("textbox", { name: "Rename Launch" }) as HTMLInputElement;
    field.value = "ローンチ";
    fireEvent.keyDown(field, { key: "Enter", isComposing: true });
    fireEvent.keyDown(field, { key: "Enter", keyCode: 229 });
    expect(mocks.rename).not.toHaveBeenCalled();
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

  it("says the chats could not be read, never that there are none, and tries again", () => {
    mocks.conversations.mockReturnValue([]);
    mocks.listFailed.mockReturnValue(true);

    const { getByText, queryByText, getByRole } = render(() => <ConversationList onPick={() => undefined} />);

    expect(getByText("Could not read your chats.")).toBeTruthy();
    expect(queryByText("No chats yet.")).toBeNull();
    fireEvent.click(getByRole("button", { name: "Try again" }));
    expect(mocks.refreshList).toHaveBeenCalledTimes(1);
  });
});
