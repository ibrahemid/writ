import { createSignal, createRoot } from "solid-js";
import { promptFillPlaceholders, promptEstimateTokens } from "../../services/tauri";

// Singleton state — Writ is single-window: one prompt-fill modal, one estimate.
// Estimates the *filled* prompt: it fills the template with the current values
// (authoritative Rust substitution) and then counts tokens, so the workbench
// reflects what actually gets copied.

const DEBOUNCE_MS = 400;

function createPromptEstimateStore() {
  const [count, setCount] = createSignal<number | null>(null);
  // The filled text the count was taken from: the modal's preview shows what
  // Copy will put on the clipboard.
  const [filled, setFilled] = createSignal<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;

  function request(template: string, values: Record<string, string>): void {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    const requested = ++generation;
    if (template.trim().length === 0) {
      setCount(null);
      setFilled(null);
      return;
    }
    timer = setTimeout(() => {
      timer = null;
      promptFillPlaceholders(template, values)
        .then((text) => {
          if (requested === generation) setFilled(text);
          return promptEstimateTokens(text);
        })
        .then((estimate) => {
          if (requested === generation) setCount(estimate);
        })
        .catch(() => {
          if (requested === generation) {
            setCount(null);
            setFilled(null);
          }
        });
    }, DEBOUNCE_MS);
  }

  function reset(): void {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    generation += 1;
    setCount(null);
    setFilled(null);
  }

  return { count, filled, request, reset };
}

export const promptEstimateStore = createRoot(createPromptEstimateStore);
