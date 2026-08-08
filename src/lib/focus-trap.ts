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

const SILENT_ATTR = "data-writ-focus-silent";

function matchesFocusVisible(el: Element | null): boolean {
  if (!el) return false;
  try {
    return el.matches(":focus-visible");
  } catch {
    // Engine cannot evaluate :focus-visible — never suppress a ring we cannot prove is unwanted.
    return true;
  }
}

function restoreFocusQuietly(el: HTMLElement): void {
  el.setAttribute(SILENT_ATTR, "");
  const controller = new AbortController();
  const clear = () => {
    el.removeAttribute(SILENT_ATTR);
    controller.abort();
  };
  el.addEventListener("keydown", clear, { signal: controller.signal });
  el.addEventListener("focusout", clear, { signal: controller.signal });
  el.focus();
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
  // Must be read before the trap moves focus — afterwards the origin reports focus-visible.
  // An origin still carrying the silent attribute was focused by a previous quiet restore,
  // so its focus-visible match is synthetic and must not promote this teardown to the loud path.
  const originWasFocusVisible =
    matchesFocusVisible(previouslyFocused) &&
    !(previouslyFocused instanceof Element && previouslyFocused.hasAttribute(SILENT_ATTR));
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
      if (originWasFocusVisible) previouslyFocused.focus();
      else restoreFocusQuietly(previouslyFocused);
    } else {
      const fallback = opts.fallbackRestore?.() ?? null;
      if (isRestorable(fallback)) fallback.focus();
    }
  };
}
