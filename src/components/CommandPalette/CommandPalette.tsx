import { createSignal, For, Show, createMemo, createEffect } from "solid-js";
import { getAllCommands } from "../../commands/registry";
import type { Command } from "../../types/commands";
import "./CommandPalette.css";

// Singleton state — Writ is single-window, single-instance per component
const [isOpen, setIsOpen] = createSignal(false);

export function openCommandPalette() { setIsOpen(true); }
export function closeCommandPalette() { setIsOpen(false); }
export function toggleCommandPalette() { setIsOpen(prev => !prev); }

export default function CommandPalette() {
  const [query, setQuery] = createSignal("");
  let inputRef: HTMLInputElement | undefined;

  const filtered = createMemo(() => {
    const q = query().toLowerCase();
    const all = getAllCommands();
    if (!q) return all;
    return all.filter(cmd =>
      cmd.label.toLowerCase().includes(q) || cmd.id.toLowerCase().includes(q)
    );
  });

  createEffect(() => {
    if (isOpen() && inputRef) {
      requestAnimationFrame(() => {
        inputRef!.focus();
      });
    }
  });

  function handleSelect(cmd: Command) {
    cmd.execute();
    setIsOpen(false);
    setQuery("");
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      setIsOpen(false);
      setQuery("");
    }
  }

  return (
    <Show when={isOpen()}>
      <div class="palette-overlay" onClick={() => { setIsOpen(false); setQuery(""); }}>
        <div class="palette" onClick={(e) => e.stopPropagation()}>
          <input
            ref={inputRef}
            type="text"
            class="palette-input"
            placeholder="Type a command..."
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={handleKeyDown}
          />
          <div class="palette-results">
            <For each={filtered()}>
              {(cmd) => (
                <button class="palette-item" onClick={() => handleSelect(cmd)}>
                  <span class="palette-item-label">{cmd.label}</span>
                  {cmd.keybinding && (
                    <span class="palette-item-key">{cmd.keybinding}</span>
                  )}
                </button>
              )}
            </For>
          </div>
        </div>
      </div>
    </Show>
  );
}
