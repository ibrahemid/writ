import { getFirstFocusable, getFocusableWithin } from "./focusable";
import { pushModal, popModal } from "./modal-stack";

export interface FocusTrapOptions {
  onEscape?: () => void;
  isActive?: () => boolean;
  fallbackRestore?: () => HTMLElement | null;
}

function isRestorable(el: Element | null): el is HTMLElement {
  if (!el) return false;
  if (!(el instanceof HTMLElement)) return false;
  if (!el.isConnected) return false;
  if (el === document.body) return false;
  return true;
}

function collectPathInerts(container: HTMLElement): HTMLElement[] {
  const inerted: HTMLElement[] = [];
  let node: HTMLElement | null = container;
  while (node && node !== document.body) {
    const parent: HTMLElement | null = node.parentElement;
    if (!parent) break;
    for (const child of Array.from(parent.children)) {
      if (child === node) continue;
      if (!(child instanceof HTMLElement)) continue;
      if (child.hasAttribute("inert")) continue;
      child.setAttribute("inert", "");
      inerted.push(child);
    }
    node = parent;
  }
  return inerted;
}

export function installFocusTrap(
  container: HTMLElement,
  opts: FocusTrapOptions = {},
): () => void {
  const previouslyFocused = document.activeElement;
  const inertedPeers = collectPathInerts(container);
  pushModal();
  let popped = false;

  if (!container.contains(previouslyFocused)) {
    const first = getFirstFocusable(container);
    if (first) {
      first.focus();
    } else {
      if (!container.hasAttribute("tabindex")) container.setAttribute("tabindex", "-1");
      container.focus();
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (opts.isActive && !opts.isActive()) return;
    if (e.key === "Escape") {
      if (opts.onEscape) {
        e.preventDefault();
        e.stopPropagation();
        opts.onEscape();
      }
      return;
    }
    if (e.key !== "Tab") return;
    const list = getFocusableWithin(container);
    if (list.length === 0) {
      e.preventDefault();
      return;
    }
    const active = document.activeElement as HTMLElement | null;
    const idx = active ? list.indexOf(active) : -1;
    let nextIdx: number;
    if (e.shiftKey) {
      nextIdx = idx <= 0 ? list.length - 1 : idx - 1;
    } else {
      nextIdx = idx === -1 || idx === list.length - 1 ? 0 : idx + 1;
    }
    e.preventDefault();
    list[nextIdx].focus();
  }

  container.addEventListener("keydown", onKeyDown);

  return () => {
    container.removeEventListener("keydown", onKeyDown);
    for (const el of inertedPeers) el.removeAttribute("inert");
    if (!popped) {
      popModal();
      popped = true;
    }
    if (isRestorable(previouslyFocused)) {
      previouslyFocused.focus();
    } else {
      const fallback = opts.fallbackRestore?.() ?? null;
      if (isRestorable(fallback)) fallback.focus();
    }
  };
}
