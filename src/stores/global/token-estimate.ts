import { createSignal, createRoot } from "solid-js";
import { promptEstimateTokens } from "../../services/tauri";

// Singleton state — Writ is single-window: one status bar, one estimate.

const DEBOUNCE_MS = 500;

export function formatTokenCount(count: number): string {
  if (count < 1000) return String(count);
  const thousands = count / 1000;
  if (thousands < 10) {
    const rounded = Math.round(thousands * 10) / 10;
    if (rounded >= 10) return "10k";
    return Number.isInteger(rounded) ? `${rounded}k` : `${rounded.toFixed(1)}k`;
  }
  return `${Math.round(thousands)}k`;
}

function createTokenEstimateStore() {
  const [count, setCount] = createSignal<number | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;

  function request(text: string): void {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    const requested = ++generation;
    if (text.trim().length === 0) {
      setCount(null);
      return;
    }
    timer = setTimeout(() => {
      timer = null;
      promptEstimateTokens(text).then(
        (estimate) => {
          if (requested === generation) setCount(estimate);
        },
        () => {
          if (requested === generation) setCount(null);
        },
      );
    }, DEBOUNCE_MS);
  }

  function reset(): void {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    generation += 1;
    setCount(null);
  }

  return { count, request, reset };
}

export const tokenEstimateStore = createRoot(createTokenEstimateStore);
