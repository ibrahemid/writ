import { createRoot, createSignal } from "solid-js";

// Singleton state — Jotra is single-window
const { isOpen, setOpen } = createRoot(() => {
  const [isOpen, setOpen] = createSignal(false);
  return { isOpen, setOpen };
});

export function setContextMenuOpen(open: boolean): void {
  setOpen(open);
}

export function isContextMenuOpen(): boolean {
  return isOpen();
}
