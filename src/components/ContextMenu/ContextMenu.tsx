import { createSignal, createEffect, Show, For, onCleanup } from "solid-js";
import "./ContextMenu.css";

export interface MenuItem {
  label: string;
  action: () => void;
  danger?: boolean;
  /**
   * Draws a divider *above* this item. Purely presentational: the item is still
   * a normal, clickable entry. It is not "this row is a separator" — reading it
   * that way silently killed every item that opened a group (Spelling settings,
   * Close All Tabs, Clear All History).
   */
  separator?: boolean;
  /** The only flag that makes an item non-actionable. */
  disabled?: boolean;
  /** Shortcut hint, right-aligned. */
  kbd?: string;
}

// A menu is positioned either at cursor coordinates (right-click) or anchored
// to an element's rect (a status-bar chip, opening upward). Both modes share
// the same keyboard navigation and dismissal.
interface ContextMenuState {
  items: MenuItem[];
  cursor?: { x: number; y: number };
  anchor?: DOMRect;
  trigger?: HTMLElement | null;
}

// Singleton state — Writ is single-window, single-instance per component
const [menu, setMenu] = createSignal<ContextMenuState | null>(null);

export function showContextMenu(x: number, y: number, items: MenuItem[]) {
  setMenu({ items, cursor: { x, y } });
}

/**
 * Opens the menu anchored above `anchor`. When `trigger` is given, focus
 * returns to it on close, so keyboard users land back where they started.
 */
export function showAnchoredMenu(anchor: DOMRect, items: MenuItem[], trigger?: HTMLElement) {
  setMenu({ items, anchor, trigger: trigger ?? null });
}

export function hideContextMenu() {
  setMenu(null);
}

/** Keeps the menu off the very edge of the window when it has to be clamped. */
const EDGE_GAP = 4;

export default function ContextMenu() {
  const [focused, setFocused] = createSignal(-1);
  // Measured after render, so clamping uses the real box rather than a guess.
  const [size, setSize] = createSignal({ width: 0, height: 0 });
  let menuRef: HTMLDivElement | undefined;
  let buttons: (HTMLButtonElement | undefined)[] = [];

  function handleClickOutside() {
    close();
  }

  function close() {
    const trigger = menu()?.trigger;
    setMenu(null);
    setFocused(-1);
    buttons = [];
    trigger?.focus();
  }

  function focusableIndices(items: MenuItem[]): number[] {
    return items.map((it, i) => (it.disabled ? -1 : i)).filter((i) => i >= 0);
  }

  function moveFocus(delta: number) {
    const m = menu();
    if (!m) return;
    const order = focusableIndices(m.items);
    if (order.length === 0) return;
    const current = order.indexOf(focused());
    const next = current === -1 ? 0 : (current + delta + order.length) % order.length;
    setFocused(order[next]);
  }

  function activate(index: number) {
    const m = menu();
    const item = m?.items[index];
    if (!item || item.disabled) return;
    item.action();
    close();
  }

  function onKeyDown(event: KeyboardEvent) {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        moveFocus(1);
        break;
      case "ArrowUp":
        event.preventDefault();
        moveFocus(-1);
        break;
      case "Home":
        event.preventDefault();
        setFocused(focusableIndices(menu()?.items ?? [])[0] ?? -1);
        break;
      case "End": {
        event.preventDefault();
        const order = focusableIndices(menu()?.items ?? []);
        setFocused(order[order.length - 1] ?? -1);
        break;
      }
      case "Enter":
      case " ":
        event.preventDefault();
        if (focused() >= 0) activate(focused());
        break;
      case "Escape":
        event.preventDefault();
        close();
        break;
      case "Tab":
        close();
        break;
    }
  }

  // Focus the first actionable item when the menu opens, so arrow keys work at once.
  createEffect(() => {
    const m = menu();
    if (!m) return;
    const first = focusableIndices(m.items)[0] ?? -1;
    setFocused(first);
  });

  // Measure once per open. The first paint may overflow the viewport by up to
  // one frame; re-reading the box then re-positions it.
  createEffect(() => {
    if (!menu()) {
      setSize({ width: 0, height: 0 });
      return;
    }
    const el = menuRef;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setSize((prev) =>
      prev.width === rect.width && prev.height === rect.height
        ? prev
        : { width: rect.width, height: rect.height },
    );
  });

  // Move DOM focus to follow the focused index.
  createEffect(() => {
    const index = focused();
    if (index < 0) return;
    requestAnimationFrame(() => buttons[index]?.focus());
  });

  // Register the outside-click dismisser only while the menu is open, and only
  // after the opening event has finished propagating. Solid delegates clicks at
  // the document, so a chip's onClick opens the menu during the same click that
  // a document-level listener would then read as "outside" and close instantly.
  // Deferring registration past the current event loop tick lets the opening
  // click complete first; the next click (a genuine outside click) dismisses.
  createEffect(() => {
    if (!menu()) return;
    let registered = false;
    const timer = setTimeout(() => {
      document.addEventListener("click", handleClickOutside);
      registered = true;
    }, 0);
    onCleanup(() => {
      clearTimeout(timer);
      if (registered) document.removeEventListener("click", handleClickOutside);
    });
  });

  function positionStyle(m: ContextMenuState): Record<string, string> {
    if (m.anchor) {
      // Open upward from the anchor's top edge (status bar sits at the bottom).
      return {
        left: `${clampLeft(m.anchor.left)}px`,
        bottom: `${Math.max(EDGE_GAP, window.innerHeight - m.anchor.top + 4)}px`,
      };
    }
    // Cursor mode: flip back over the cursor when the menu would overflow, so a
    // right-click near the right or bottom edge stays fully on screen.
    const x = m.cursor?.x ?? 0;
    const y = m.cursor?.y ?? 0;
    const width = size().width;
    const height = size().height;
    const left =
      x + width + EDGE_GAP > window.innerWidth ? Math.max(EDGE_GAP, x - width) : x;
    const top =
      y + height + EDGE_GAP > window.innerHeight ? Math.max(EDGE_GAP, y - height) : y;
    return { left: `${clampLeft(left)}px`, top: `${top}px` };
  }

  function clampLeft(left: number): number {
    const max = window.innerWidth - size().width - EDGE_GAP;
    return Math.max(EDGE_GAP, Math.min(left, Math.max(EDGE_GAP, max)));
  }

  return (
    <Show when={menu()}>
      {(m) => (
        <div
          ref={(el) => (menuRef = el)}
          class="context-menu"
          role="menu"
          style={positionStyle(m())}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={onKeyDown}
        >
          <For each={m().items}>
            {(item, index) => (
              <>
                {item.separator && <div class="context-menu-separator" />}
                <button
                  ref={(el) => (buttons[index()] = el)}
                  type="button"
                  role="menuitem"
                  tabindex={-1}
                  disabled={item.disabled}
                  class={`context-menu-item ${item.danger ? "context-menu-danger" : ""}`}
                  onClick={() => activate(index())}
                >
                  <span class="context-menu-label">{item.label}</span>
                  {item.kbd && <span class="context-menu-kbd">{item.kbd}</span>}
                </button>
              </>
            )}
          </For>
        </div>
      )}
    </Show>
  );
}
