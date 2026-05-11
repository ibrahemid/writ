import { createSignal, For, Show, createMemo, createEffect } from "solid-js";
import { getAllCommands } from "../../commands/registry";
import type { Command } from "../../types/commands";
import Kbd from "../Kbd/Kbd";
import "./CommandPalette.css";

// Singleton state — Writ is single-window, single-instance per component
const [isOpen, setIsOpen] = createSignal(false);

export function openCommandPalette() { setIsOpen(true); }
export function closeCommandPalette() { setIsOpen(false); }
export function toggleCommandPalette() { setIsOpen(prev => !prev); }

export default function CommandPalette() {
  const [query, setQuery] = createSignal("");
  const [selectedIndex, setSelectedIndex] = createSignal(0);
  let inputRef: HTMLInputElement | undefined;
  let listRef: HTMLDivElement | undefined;

  const filtered = createMemo(() => {
    const q = query().toLowerCase().trim();
    const visible = getAllCommands().filter(cmd => cmd.scope === "app");
    if (!q) return visible;
    return visible.filter(cmd =>
      cmd.label.toLowerCase().includes(q) ||
      cmd.id.toLowerCase().includes(q) ||
      (cmd.description?.toLowerCase().includes(q) ?? false)
    );
  });

  createEffect(() => {
    filtered();
    setSelectedIndex(0);
  });

  createEffect(() => {
    if (isOpen() && inputRef) {
      requestAnimationFrame(() => {
        inputRef!.focus();
      });
    } else if (!isOpen()) {
      setQuery("");
      setSelectedIndex(0);
    }
  });

  function handleSelect(cmd: Command) {
    cmd.execute();
    setIsOpen(false);
  }

  function handleKeyDown(e: KeyboardEvent) {
    const list = filtered();
    if (e.key === "Escape") {
      setIsOpen(false);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex(i => Math.min(i + 1, list.length - 1));
      scrollSelectedIntoView();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex(i => Math.max(i - 1, 0));
      scrollSelectedIntoView();
    } else if (e.key === "Enter") {
      e.preventDefault();
      const cmd = list[selectedIndex()];
      if (cmd) handleSelect(cmd);
    }
  }

  function scrollSelectedIntoView() {
    requestAnimationFrame(() => {
      if (!listRef) return;
      const el = listRef.querySelector<HTMLElement>(".palette-item.is-selected");
      el?.scrollIntoView({ block: "nearest" });
    });
  }

  return (
    <Show when={isOpen()}>
      <div class="palette-overlay" onClick={() => setIsOpen(false)}>
        <div class="palette" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Command palette">
          <input
            ref={inputRef}
            type="text"
            class="palette-input"
            placeholder="Search commands"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={handleKeyDown}
            aria-label="Command search"
          />
          <Show
            when={filtered().length > 0}
            fallback={
              <div class="palette-empty">
                <div class="palette-empty-title">Nothing matches "{query()}"</div>
                <div class="palette-empty-hint">Try a different word, or press Esc to dismiss.</div>
              </div>
            }
          >
            <div class="palette-results" ref={listRef}>
              <For each={filtered()}>
                {(cmd, i) => (
                  <button
                    type="button"
                    class={`palette-item ${selectedIndex() === i() ? "is-selected" : ""}`}
                    onClick={() => handleSelect(cmd)}
                    onMouseMove={() => setSelectedIndex(i())}
                  >
                    <div class="palette-item-text">
                      <span class="palette-item-label">{cmd.label}</span>
                      <Show when={cmd.description}>
                        <span class="palette-item-desc">{cmd.description}</span>
                      </Show>
                    </div>
                    <Kbd binding={cmd.keybinding} muted={!cmd.keybinding} />
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
      </div>
    </Show>
  );
}
