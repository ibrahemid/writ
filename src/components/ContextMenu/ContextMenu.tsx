import { createSignal, createEffect, Show, For, onCleanup } from "solid-js";
import Icon, { type IconName } from "../Icon/Icon";
import "./ContextMenu.css";

export interface MenuItem {
  label: string;
  action: () => void;
  danger?: boolean;
  /** Leading glyph. Omit it and the row is text only. */
  icon?: IconName;
  /**
   * Draws a divider *above* this item. Purely presentational: the item is still
   * a normal, clickable entry. It is not "this row is a separator" — reading it
   * that way silently killed every item that opened a group (Spelling settings,
   * Close all tabs, Clear all history).
   */
  separator?: boolean;
  /** The only flag that makes an item non-actionable. */
  disabled?: boolean;
  /** Shortcut hint, right-aligned. */
  kbd?: string;
}

// A menu is positioned either at cursor coordinates (right-click) or anchored
// to an element's rect (a status-bar chip, or a word in the editor). Both modes
// share the same keyboard navigation and dismissal.
interface ContextMenuState {
  items: MenuItem[];
  cursor?: { x: number; y: number };
  anchor?: DOMRect;
  trigger?: HTMLElement | null;
  /** Region the menu must stay inside. Defaults to the viewport. */
  bounds?: DOMRect;
}

// Singleton state — Writ is single-window, single-instance per component
const [menu, setMenu] = createSignal<ContextMenuState | null>(null);

export function showContextMenu(
  x: number,
  y: number,
  items: MenuItem[],
  bounds?: DOMRect,
) {
  setMenu({ items, cursor: { x, y }, bounds });
}

/**
 * Opens the menu against `anchor`, preferring above it and flipping below when
 * there is not room. When `trigger` is given, focus returns to it on close, so
 * keyboard users land back where they started.
 *
 * `bounds` confines the menu to a region — the editor's scroller, say — so a
 * word near the top of the document does not open a menu over the tab bar.
 */
export function showAnchoredMenu(
  anchor: DOMRect,
  items: MenuItem[],
  trigger?: HTMLElement,
  bounds?: DOMRect,
) {
  setMenu({ items, anchor, trigger: trigger ?? null, bounds });
}

export function hideContextMenu() {
  setMenu(null);
}

/** Keeps the menu off the very edge of its allowed region. */
const EDGE_GAP = 4;
/** Space between the menu and the thing it is anchored to. */
const GAP = 4;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

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

  // Focus does not always land in the menu (a spelling popover opens on a
  // double-click the editor also handles, and the editor keeps the caret), so
  // the menu's own key handler never sees Escape. At the document level:
  // Escape closes; any other key outside the menu means the user has moved on,
  // so the menu closes and the key goes wherever it was headed.
  function handleDocumentKeyDown(event: KeyboardEvent) {
    if (menuRef && event.target instanceof Node && menuRef.contains(event.target)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key.length === 1 || event.key === "Backspace" || event.key === "Enter") close();
  }

  createEffect(() => {
    if (!menu()) return;
    document.addEventListener("keydown", handleDocumentKeyDown, true);
    onCleanup(() => document.removeEventListener("keydown", handleDocumentKeyDown, true));
  });

  /** The region the menu may occupy: the caller's bounds, else the viewport. */
  function limits(m: ContextMenuState) {
    return {
      top: (m.bounds?.top ?? 0) + EDGE_GAP,
      bottom: (m.bounds?.bottom ?? window.innerHeight) - EDGE_GAP,
      left: (m.bounds?.left ?? 0) + EDGE_GAP,
      right: (m.bounds?.right ?? window.innerWidth) - EDGE_GAP,
    };
  }

  function positionStyle(m: ContextMenuState): Record<string, string> {
    const box = limits(m);
    const height = size().height;

    if (m.anchor) {
      // Prefer above (the status-bar chips sit at the bottom of the window), but
      // flip below when there is no room — a misspelled word on the first line
      // would otherwise open its corrections over the tab bar.
      const fitsAbove = m.anchor.top - GAP - height >= box.top;
      const top = fitsAbove ? m.anchor.top - GAP - height : m.anchor.bottom + GAP;
      return {
        left: `${clampLeft(clamp(m.anchor.left, box.left, box.right - size().width), m)}px`,
        top: `${clamp(top, box.top, Math.max(box.top, box.bottom - height))}px`,
      };
    }

    // Cursor mode: flip back over the cursor when the menu would overflow, so a
    // right-click near the right or bottom edge stays fully on screen.
    const x = m.cursor?.x ?? 0;
    const y = m.cursor?.y ?? 0;
    const width = size().width;
    const left = x + width > box.right ? x - width : x;
    const top = y + height > box.bottom ? y - height : y;
    return {
      left: `${clampLeft(left, m)}px`,
      top: `${clamp(top, box.top, Math.max(box.top, box.bottom - height))}px`,
    };
  }

  function clampLeft(left: number, m: ContextMenuState): number {
    const box = limits(m);
    return clamp(left, box.left, Math.max(box.left, box.right - size().width));
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
                  {item.icon && <Icon name={item.icon} />}
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
