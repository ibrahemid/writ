import { createSignal, createMemo, Show, For, createEffect, onCleanup } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { useWindow } from "../WindowProvider/WindowProvider";
import { promptEstimateStore } from "../../stores/global/prompt-estimate";
import { formatTokenCount } from "../../stores/global/token-estimate";
import "./PromptFillModal.css";

export type PlaceholderValues = Record<string, string>;

interface PendingFill {
  names: string[];
  resolve: (values: PlaceholderValues | null) => void;
}

// Singleton state — Writ is single-window, single-instance per component
const [pending, setPending] = createSignal<PendingFill | null>(null);
const [values, setValues] = createSignal<PlaceholderValues>({});
const [template, setTemplate] = createSignal("");

export function requestPlaceholderFill(
  names: string[],
  templateText = "",
): Promise<PlaceholderValues | null> {
  return new Promise<PlaceholderValues | null>((resolve) => {
    setPending((prev) => {
      if (prev) prev.resolve(null);
      setValues(Object.fromEntries(names.map((name) => [name, ""])));
      setTemplate(templateText);
      return { names, resolve };
    });
  });
}

function settle(confirmed: boolean) {
  const current = pending();
  if (!current) return;
  current.resolve(confirmed ? values() : null);
  setPending(null);
  setValues({});
  setTemplate("");
  promptEstimateStore.reset();
}

export default function PromptFillModal() {
  const win = useWindow();
  let dialogRef: HTMLDivElement | undefined;

  const varCount = createMemo(() => pending()?.names.length ?? 0);

  // Re-estimate the filled prompt whenever the template or any value changes.
  // The store debounces and runs the authoritative Rust fill before counting.
  createEffect(() => {
    if (!pending()) return;
    promptEstimateStore.request(template(), values());
  });

  createEffect(() => {
    const current = pending();
    if (!current || !dialogRef) return;
    const teardown = installFocusTrap(dialogRef, {
      onEscape: () => settle(false),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  return (
    <Show when={pending()}>
      {(req) => (
        <div class="placeholders-overlay" onClick={() => settle(false)}>
          <div
            ref={dialogRef}
            class="placeholders-dialog"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-labelledby="placeholders-title"
          >
            <div id="placeholders-title" class="placeholders-title">
              Fill Placeholders
            </div>
            <div class="placeholders-meta" aria-live="polite">
              <span class="placeholders-meta-count">
                {varCount()} {varCount() === 1 ? "variable" : "variables"}
              </span>
              <Show when={promptEstimateStore.count() !== null}>
                <span class="placeholders-meta-sep" aria-hidden="true">
                  ·
                </span>
                <span class="placeholders-meta-tokens">
                  ~{formatTokenCount(promptEstimateStore.count()!)} tokens
                </span>
              </Show>
            </div>
            <Show
              when={req().names.length > 0}
              fallback={<div class="placeholders-empty">No placeholders found</div>}
            >
              <form
                class="placeholders-fields"
                onSubmit={(e) => {
                  e.preventDefault();
                  settle(true);
                }}
              >
                <For each={req().names}>
                  {(name) => (
                    <div class="placeholders-field">
                      <label class="placeholders-label" for={`placeholder-${name}`}>
                        {`{{${name}}}`}
                      </label>
                      <input
                        id={`placeholder-${name}`}
                        class="placeholders-input"
                        type="text"
                        autocomplete="off"
                        value={values()[name] ?? ""}
                        onInput={(e) =>
                          setValues((prev) => ({ ...prev, [name]: e.currentTarget.value }))
                        }
                      />
                    </div>
                  )}
                </For>
              </form>
            </Show>
            <div class="placeholders-actions">
              <Show when={req().names.length > 0}>
                <button
                  type="button"
                  class="placeholders-button placeholders-cancel"
                  onClick={() => settle(false)}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  class="placeholders-button placeholders-confirm"
                  onClick={() => settle(true)}
                >
                  Copy filled text
                </button>
              </Show>
              <Show when={req().names.length === 0}>
                <button
                  type="button"
                  class="placeholders-button placeholders-confirm"
                  onClick={() => settle(false)}
                >
                  Close
                </button>
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
