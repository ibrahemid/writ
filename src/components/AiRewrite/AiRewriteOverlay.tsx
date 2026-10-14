import { For, Show, createEffect, onCleanup } from "solid-js";
import { aiRewriteStore } from "../../stores/global/ai-rewrite";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import "./AiRewriteOverlay.css";
import { isImeComposing } from "../../lib/ime";
import { formatKeybinding } from "../../lib/keybinding-format";

/** The chord that applies a finished rewrite and retries a failed one. */
const RUN_CHORD = "CmdOrCtrl+Enter";

const STATUS_LABELS: Record<string, string> = {
  streaming: "Streaming…",
  done: "Ready to apply",
  error: "Could not rewrite",
};

export default function AiRewriteOverlay() {
  const store = aiRewriteStore;
  let instructionInput: HTMLInputElement | undefined;

  // Escape discards and Cmd/Ctrl+Enter applies a finished rewrite or retries
  // a failed one, wherever focus sits — the editor keeps focus so edits can
  // still abort the preview. Intercept only these two chords so typing (and
  // abort-on-edit) still flows through.
  createEffect(() => {
    if (!store.isOpen()) return;
    function onKeyDown(e: KeyboardEvent) {
      if (isImeComposing(e)) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        store.discard();
      } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && store.status() === "done") {
        e.preventDefault();
        e.stopPropagation();
        store.apply();
      } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && store.status() === "error") {
        e.preventDefault();
        e.stopPropagation();
        store.retry();
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
    if (isImeComposing(e)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      store.submitInstruction();
    }
  }

  return (
    <Show when={store.isOpen()}>
      {/* A region rather than a dialog: the editor keeps focus so an edit can
          abort the preview, and a dialog that never takes focus is not
          reachable in a screen reader's browse mode. */}
      <div class="ai-overlay" role="region" aria-label="Rewrite preview">
        <div class="ai-overlay-header">
          <span class="ai-overlay-action">{store.actionLabel()}</span>
          <span class="ai-overlay-status" data-status={store.status()} aria-live="polite">
            {STATUS_LABELS[store.status()] ?? ""}
          </span>
          <Tooltip label="Discard rewrite" chord="Escape">
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

        {/* A finished rewrite is read as one text with its changes marked in
            place, so a proofread that touched three words shows three words
            (CH-09). The two texts side by side stay for a reply still
            arriving, a failure, and a comparison that was refused. */}
        <Show when={store.status() === "done" ? store.changes() : null}>
          {(spans) => (
            <div class="ai-overlay-pane" aria-live="polite">
              <div class="ai-overlay-pane-label">Changes</div>
              <div class="ai-overlay-pane-body ai-overlay-result ai-overlay-changes">
                <For each={spans()}>
                  {(span) =>
                    span.kind === "added" ? (
                      <ins class="ai-overlay-added">{span.text}</ins>
                    ) : span.kind === "removed" ? (
                      <del class="ai-overlay-removed">{span.text}</del>
                    ) : (
                      <span>{span.text}</span>
                    )
                  }
                </For>
              </div>
            </div>
          )}
        </Show>

        <Show
          when={
            store.status() !== "awaiting-instruction" &&
            !(store.status() === "done" && store.changes() !== null)
          }
        >
          <div class="ai-overlay-panes">
            <div class="ai-overlay-pane">
              <div class="ai-overlay-pane-label">Original</div>
              <div class="ai-overlay-pane-body">{store.original()}</div>
            </div>
            <div class="ai-overlay-pane" aria-live="polite">
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
            <Button
              variant="primary"
              aria-keyshortcuts="Meta+Enter Control+Enter"
              onClick={() => store.retry()}
            >
              Retry
              <span class="ai-overlay-chord" aria-hidden="true">
                {formatKeybinding(RUN_CHORD)}
              </span>
            </Button>
          </Show>
          <Show when={store.status() === "done"}>
            <Button onClick={() => store.discard()}>Discard</Button>
            <Button
              variant="primary"
              aria-keyshortcuts="Meta+Enter Control+Enter"
              onClick={() => store.apply()}
            >
              Apply
              <span class="ai-overlay-chord" aria-hidden="true">
                {formatKeybinding(RUN_CHORD)}
              </span>
            </Button>
          </Show>
        </div>
      </div>
    </Show>
  );
}
