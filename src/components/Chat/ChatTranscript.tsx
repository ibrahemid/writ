import { For, Show, createEffect } from "solid-js";
import Button from "../Button/Button";
import ChatTurn from "./ChatTurn";
import { chatStore } from "../../stores/global/chat";

/** The conversation, oldest turn first. */
export default function ChatTranscript() {
  // The newest turn is the one being read. Streamed text appends into the last
  // message, so this follows it down; it moves the scroll position and nothing
  // else, which is what a reduced-motion setting asks of a live region.
  let scroller: HTMLDivElement | undefined;
  createEffect(() => {
    const messages = chatStore.messages();
    // Read so a frame that only lengthens the last message still moves it.
    chatStore.status();
    if (!scroller || messages.length === 0) return;
    scroller.scrollTop = scroller.scrollHeight;
  });

  const last = () => chatStore.messages().length - 1;

  return (
    <div class="chat-transcript" ref={scroller}>
      <Show
        when={chatStore.messages().length > 0}
        fallback={
          <p class="chat-empty">
            Ask about a note. Apply the change an answer offers, or discard it.
          </p>
        }
      >
        <For each={chatStore.messages()}>
          {(message, index) => (
            <ChatTurn
              message={message}
              thinking={
                chatStore.status() === "thinking" &&
                index() === last() &&
                message.role === "assistant"
              }
            />
          )}
        </For>
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
    </div>
  );
}
