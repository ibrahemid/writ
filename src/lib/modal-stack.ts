import { createSignal, createRoot } from "solid-js";

const { openCount, setOpenCount } = createRoot(() => {
  const [openCount, setOpenCount] = createSignal(0);
  return { openCount, setOpenCount };
});

export function pushModal(): void {
  setOpenCount((n) => n + 1);
}

export function popModal(): void {
  setOpenCount((n) => Math.max(0, n - 1));
}

export function isModalOpen(): boolean {
  return openCount() > 0;
}

export function modalOpenCount(): number {
  return openCount();
}

export function resetModalStack(): void {
  setOpenCount(0);
}
