import { Show, createEffect, onCleanup } from "solid-js";
import { aiRewriteStore } from "../../stores/global/ai-rewrite";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import "./AiRewriteOverlay.css";

const STATUS_LABELS: Record<string, string> = {
  streaming: "Streaming…",
  done: "Ready to apply",
  error: "Failed",
};

export default function AiRewriteOverlay() {
  const store = aiRewriteStore;
  let instructionInput: HTMLInputElement | undefined;

  // Escape discards and Cmd/Ctrl+Enter applies, wherever focus sits — the
  // editor keeps focus so edits can still abort the preview. Intercept only
  // these two chords so typing (and abort-on-edit) still flows through.
  createEffect(() => {
    if (!store.isOpen()) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        store.discard();
      } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && store.status() === "done") {
        e.preventDefault();
        e.stopPropagation();
        store.apply();
      }
    }
    window.addEventListener("keydown", onKeyDown, true);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown, true));
  });

  createEffect(() => {
    if (store.status() === "awaiting-instruction" && instructionInput) {
      const el = instructionInput;
      requestAnimationFrame(() => el.focus());
    }
  });

  function onInstructionKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      store.submitInstruction();
    }
  }

  return (
    <Show when={store.isOpen()}>
      <div class="ai-overlay" role="dialog" aria-label="Rewrite preview">
        <div class="ai-overlay-header">
          <span class="ai-overlay-action">{store.actionLabel()}</span>
          <span class="ai-overlay-status" data-status={store.status()} aria-live="polite">
            {STATUS_LABELS[store.status()] ?? ""}
          </span>
          <Tooltip label="Discard rewrite">
            <Button
              variant="ghost"
              icon="x"
              iconSize={14}
              onClick={() => store.discard()}
              aria-label="Discard rewrite"
            />
          </Tooltip>
        </div>

        <Show when={store.status() === "awaiting-instruction"}>
          <div class="ai-overlay-instruction">
            <input
              ref={(el) => (instructionInput = el)}
              class="ai-overlay-input"
              type="text"
              spellcheck={false}
              autocomplete="off"
              placeholder="Describe the change"
              aria-label="Rewrite instruction"
              value={store.instruction()}
              onInput={(e) => store.setInstruction(e.currentTarget.value)}
              onKeyDown={onInstructionKeyDown}
            />
          </div>
        </Show>

        <Show when={store.status() !== "awaiting-instruction"}>
          <div class="ai-overlay-panes">
            <div class="ai-overlay-pane">
              <div class="ai-overlay-pane-label">Original</div>
              <div class="ai-overlay-pane-body">{store.original()}</div>
            </div>
            <div class="ai-overlay-pane">
              <div class="ai-overlay-pane-label">Result</div>
              <Show
                when={store.status() === "error"}
                fallback={
                  <div
                    class="ai-overlay-pane-body ai-overlay-result"
                    classList={{ "is-streaming": store.status() === "streaming" }}
                  >
                    {store.result()}
                  </div>
                }
              >
                <div class="ai-overlay-pane-body ai-overlay-error">{store.errorMessage()}</div>
              </Show>
            </div>
          </div>
        </Show>

        <div class="ai-overlay-footer">
          <Show when={store.status() === "awaiting-instruction"}>
            <Button onClick={() => store.discard()}>Cancel</Button>
            <Button
              variant="primary"
              disabled={store.instruction().trim().length === 0}
              onClick={() => store.submitInstruction()}
            >
              Run
            </Button>
          </Show>
          <Show when={store.status() === "streaming"}>
            <Button onClick={() => store.discard()}>Cancel</Button>
          </Show>
          <Show when={store.status() === "error"}>
            <Button onClick={() => store.discard()}>Discard</Button>
            <Button variant="primary" onClick={() => store.retry()}>
              Retry
            </Button>
          </Show>
          <Show when={store.status() === "done"}>
            <Button onClick={() => store.discard()}>Discard</Button>
            <Button variant="primary" onClick={() => store.apply()}>
              Apply
            </Button>
          </Show>
        </div>
      </div>
    </Show>
  );
}
