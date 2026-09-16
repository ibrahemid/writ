import { Index, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import Button from "../Button/Button";
import ChatTurn from "./ChatTurn";
import { chatStore } from "../../stores/global/chat";

/** How near the end still counts as being at it, in pixels. A reader who ends
 * a gesture inside this band is reading the newest text, not an earlier turn. */
const AT_BOTTOM = 48;

/** The conversation, oldest turn first. */
export default function ChatTranscript() {
  // The view follows the newest text until the reader scrolls away from it.
  // Streamed text appends into the last message many times a second, so the
  // position is written once a frame at most, and only while following: a
  // reader holding a place in an earlier turn keeps it, through the end of the
  // reply and the reload behind it. The position is assigned, never animated,
  // which is what a reduced-motion setting asks of a live region.
  let scroller: HTMLDivElement | undefined;
  let frame: number | null = null;
  const [following, setFollowing] = createSignal(true);
  const [grew, setGrew] = createSignal(false);

  function onScroll() {
    if (!scroller) return;
    const atBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight <= AT_BOTTOM;
    setFollowing(atBottom);
    if (atBottom) setGrew(false);
  }

  onMount(() => {
    scroller?.addEventListener("scroll", onScroll);
  });

  onCleanup(() => {
    scroller?.removeEventListener("scroll", onScroll);
    if (frame !== null) cancelAnimationFrame(frame);
  });

  function toBottom() {
    if (frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      if (!scroller) return;
      scroller.scrollTop = scroller.scrollHeight;
      // The write is the transcript's own, so following is re-armed here
      // rather than left to the scroll event it raises: neither that event nor
      // the text that arrived since the frame was asked for can clear it.
      setFollowing(true);
      setGrew(false);
    });
  }

  // How much the last turn held when it was last looked at, so that a frame
  // which only restates the conversation is not read as new text arriving.
  let held = 0;

  createEffect(() => {
    const messages = chatStore.messages();
    // Read so a frame that only lengthens the last message still counts.
    chatStore.status();
    if (messages.length === 0) return;
    const tail = messages[messages.length - 1];
    const size = messages.length + tail.content.length + tail.html.length;
    const longer = size > held;
    held = size;
    if (!following()) {
      if (longer) setGrew(true);
      return;
    }
    toBottom();
  });

  function toLatest() {
    setFollowing(true);
    toBottom();
  }

  const last = () => chatStore.messages().length - 1;

  /** What a reader who cannot see the column is told: the state of the reply,
   * and nothing of its text, which arrives tens of times a second and is read
   * from the log itself. */
  const liveStatus = () => {
    switch (chatStore.status()) {
      case "thinking":
      case "streaming":
        return "Reply arriving";
      case "stopped":
        return "Reply stopped";
      case "done":
        return "Reply finished";
      default:
        return "";
    }
  };

  return (
    <div class="chat-transcript" ref={scroller} role="log" tabindex="0">
      <p class="chat-transcript-status" role="status" aria-live="polite">
        {liveStatus()}
      </p>

      <Show
        when={chatStore.messages().length > 0}
        fallback={
          <p class="chat-empty">
            Ask about a note. Apply the change an answer offers, or discard it.
          </p>
        }
      >
        <Index each={chatStore.messages()}>
          {(message, index) => (
            <ChatTurn
              message={message()}
              thinking={
                chatStore.status() === "thinking" &&
                index === last() &&
                message().role === "assistant"
              }
            />
          )}
        </Index>
      </Show>

      <Show when={chatStore.status() === "error"}>
        <div class="chat-error" role="alert">
          <p class="chat-error-text">{chatStore.errorMessage()}</p>
          <Show when={chatStore.canRetry()}>
            <Button icon="arrow-u-down-left" iconSize={12} onClick={() => void chatStore.retry()}>
              Retry
            </Button>
          </Show>
        </div>
      </Show>

      <Show when={!following() && grew()}>
        <div class="chat-latest">
          <Button variant="ghost" icon="caret-down" iconSize={12} onClick={toLatest}>
            Latest
          </Button>
        </div>
      </Show>
    </div>
  );
}
