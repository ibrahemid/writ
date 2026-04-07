import { createSignal, Show, For, onMount, onCleanup } from "solid-js";
import "./ContextMenu.css";

interface MenuItem {
  label: string;
  action: () => void;
  danger?: boolean;
  separator?: boolean;
}

interface ContextMenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

// Singleton state — Writ is single-window, single-instance per component
const [menu, setMenu] = createSignal<ContextMenuState | null>(null);

export function showContextMenu(x: number, y: number, items: MenuItem[]) {
  setMenu({ x, y, items });
}

export function hideContextMenu() {
  setMenu(null);
}

export default function ContextMenu() {
  function handleClickOutside() {
    hideContextMenu();
  }

  onMount(() => {
    document.addEventListener("click", handleClickOutside);
  });

  onCleanup(() => document.removeEventListener("click", handleClickOutside));

  return (
    <Show when={menu()}>
      {(m) => (
        <div
          class="context-menu"
          style={{ left: `${m().x}px`, top: `${m().y}px` }}
          onClick={(e) => e.stopPropagation()}
        >
          <For each={m().items}>
            {(item) => (
              <>
                {item.separator && <div class="context-menu-separator" />}
                <button
                  class={`context-menu-item ${item.danger ? "context-menu-danger" : ""}`}
                  onClick={() => { item.action(); hideContextMenu(); }}
                >
                  {item.label}
                </button>
              </>
            )}
          </For>
        </div>
      )}
    </Show>
  );
}
