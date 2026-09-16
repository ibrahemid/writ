import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// Where the transcript puts the view while a reply streams. Following is the
// default; a reader who scrolls up keeps the position they chose until they
// ask for the newest text back.

const mocks = vi.hoisted(() => ({
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatRenderReply: vi.fn(),
  chatSend: vi.fn(),
  chatCancel: vi.fn(),
  chatAttachedSizes: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: mocks.chatList,
  chatOpen: mocks.chatOpen,
  chatNew: mocks.chatNew,
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: mocks.chatRenderReply,
  chatSend: mocks.chatSend,
  chatCancel: mocks.chatCancel,
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
}));

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
}));

import ChatTranscript from "../../components/Chat/ChatTranscript";
import { chatStore } from "../../stores/global/chat";
import type { ChatConversation } from "../../services/tauri";

const HEIGHT = 2000;
const VIEW = 500;

function conversation(id: string): ChatConversation {
  return {
    id,
    title: "A chat",
    created_at: "2026-09-15T10:00:00+00:00",
    updated_at: "2026-09-15T10:00:00+00:00",
    provider: "anthropic",
    model: "a-model",
    turns: [],
  };
}

/** jsdom neither lays the scroller out nor fires `scroll` on an assignment, so
 * the measurements are stubbed and every write to `scrollTop` is counted. */
function instrument(el: HTMLElement) {
  let top = 0;
  const writes: number[] = [];
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (value: number) => {
      top = value;
      writes.push(value);
    },
  });
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => HEIGHT });
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => VIEW });
  return {
    writes,
    /** The reader's own gesture: it moves the view without counting as a write. */
    readerScrollsTo(value: number) {
      top = value;
      fireEvent.scroll(el);
    },
  };
}

let frames: FrameRequestCallback[] = [];

function runFrames() {
  const queued = frames;
  frames = [];
  for (const frame of queued) frame(0);
}

beforeEach(() => {
  frames = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("cancelAnimationFrame", (handle: number) => {
    frames[handle - 1] = () => {};
  });
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.chatList.mockResolvedValue([]);
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatNew.mockResolvedValue(conversation("c1"));
  mocks.chatSend.mockResolvedValue({ conversation_id: "c1", attached: [] });
  mocks.chatAttachedSizes.mockResolvedValue([]);
  chatStore.reset();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

async function startSend() {
  chatStore.setDraft("what does it argue");
  await chatStore.send();
}

function chunk(text: string) {
  chatStore.handleStreamEvent({ conversation_id: "c1", kind: "chunk", text });
}

async function openTranscript() {
  const { container } = render(() => <ChatTranscript />);
  await startSend();
  const scroller = container.querySelector(".chat-transcript") as HTMLElement;
  return { container, scroller, view: instrument(scroller) };
}

describe("the transcript's view", () => {
  it("follows the newest text while the view is at the bottom", async () => {
    const { view } = await openTranscript();

    chunk("it argues ");
    runFrames();

    expect(view.writes).toEqual([HEIGHT]);
  });

  it("stops following once the reader scrolls up", async () => {
    const { view } = await openTranscript();

    chunk("it argues ");
    runFrames();
    view.readerScrollsTo(100);
    view.writes.length = 0;

    chunk("this and that");
    runFrames();

    expect(view.writes).toEqual([]);
  });

  it("returns to the newest text when the control is pressed", async () => {
    const { container, view } = await openTranscript();

    chunk("it argues ");
    runFrames();
    view.readerScrollsTo(100);
    chunk("this and that");
    runFrames();
    view.writes.length = 0;

    const latest = await waitFor(() => {
      const button = Array.from(container.querySelectorAll("button")).find(
        (candidate) => candidate.textContent?.trim() === "Latest",
      );
      expect(button).toBeDefined();
      return button as HTMLButtonElement;
    });
    fireEvent.click(latest);
    runFrames();

    expect(view.writes).toEqual([HEIGHT]);
  });

  it("moves the view once a frame, not once a delta", async () => {
    const { view } = await openTranscript();

    for (const text of ["one ", "two ", "three ", "four ", "five"]) chunk(text);
    runFrames();

    expect(view.writes).toHaveLength(1);
  });
});
