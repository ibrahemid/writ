import { createSignal, For, Show, createMemo, createEffect } from "solid-js";
import { useAllCommands } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import { partitionEmptyQuery, rankWithQuery } from "../../commands/ranking";
import { configStore } from "../../stores/config";
import type { Command } from "../../types/commands";
import Kbd from "../Kbd/Kbd";
import "./CommandPalette.css";

const [isOpen, setIsOpen] = createSignal(false);

export function openCommandPalette() { setIsOpen(true); }
export function closeCommandPalette() { setIsOpen(false); }
export function toggleCommandPalette() { setIsOpen(prev => !prev); }

interface PaletteSection {
  kind: "recent" | "all" | "results";
  label: string | null;
  commands: Command[];
}

export default function CommandPalette() {
  const [query, setQuery] = createSignal("");
  const [selectedIndex, setSelectedIndex] = createSignal(0);
  let inputRef: HTMLInputElement | undefined;
  let listRef: HTMLDivElement | undefined;

  const appCommands = createMemo(() =>
    useAllCommands().filter((cmd) => cmd.scope === "app"),
  );

  const sections = createMemo<PaletteSection[]>(() => {
    const q = query().trim();
    const usage = configStore.config().commands.usage;
    const all = appCommands();
    if (!q) {
      const { recent, rest } = partitionEmptyQuery(all, usage);
      const result: PaletteSection[] = [];
      if (recent.length > 0) {
        result.push({ kind: "recent", label: "Recent", commands: recent });
        result.push({ kind: "all", label: "All commands", commands: rest });
      } else {
        result.push({ kind: "all", label: null, commands: rest });
      }
      return result;
    }
    const ranked = rankWithQuery(all, q, usage);
    return [{ kind: "results", label: null, commands: ranked }];
  });

  const flat = createMemo<Command[]>(() => sections().flatMap((s) => s.commands));

  createEffect(() => {
    flat();
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
    configStore.recordCommandUse(cmd.id);
    setIsOpen(false);
  }

  function handleKeyDown(e: KeyboardEvent) {
    const list = flat();
    if (e.key === "Escape") {
      setIsOpen(false);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(i + 1, list.length - 1));
      scrollSelectedIntoView();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(i - 1, 0));
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

  function indexOf(cmd: Command): number {
    return flat().indexOf(cmd);
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
            when={flat().length > 0}
            fallback={
              <div class="palette-empty">
                <div class="palette-empty-title">Nothing matches "{query()}"</div>
                <div class="palette-empty-hint">Try a different word, or press Esc to dismiss.</div>
              </div>
            }
          >
            <div class="palette-results" ref={listRef}>
              <For each={sections()}>
                {(section) => (
                  <div class={`palette-section palette-section-${section.kind}`}>
                    <Show when={section.label}>
                      <div class="palette-section-label">{section.label}</div>
                    </Show>
                    <For each={section.commands}>
                      {(cmd) => {
                        const idx = createMemo(() => indexOf(cmd));
                        return (
                          <button
                            type="button"
                            class={`palette-item ${selectedIndex() === idx() ? "is-selected" : ""}`}
                            onClick={() => handleSelect(cmd)}
                            onMouseMove={() => setSelectedIndex(idx())}
                          >
                            <div class="palette-item-text">
                              <span class="palette-item-label">{cmd.label}</span>
                              <Show when={cmd.description}>
                                <span class="palette-item-desc">{cmd.description}</span>
                              </Show>
                            </div>
                            <Kbd
                              binding={useEffectiveBinding(cmd.id, cmd.keybinding)}
                              muted={!useEffectiveBinding(cmd.id, cmd.keybinding)}
                            />
                          </button>
                        );
                      }}
                    </For>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>
      </div>
    </Show>
  );
}
